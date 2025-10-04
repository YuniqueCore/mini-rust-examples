use std::{
    net::SocketAddr,
    str::FromStr,
    sync::{self, Arc, LazyLock, atomic::AtomicU64},
};

use anyverr::{AnyError, AnyResult};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{
        Mutex,
        mpsc::{self, UnboundedSender},
    },
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    ip: String,
    port: u16,
}

#[derive(Debug, Clone)]
pub struct AppState {
    // pub
    pub rooms: Vec<Room>,
}

impl AppState {
    pub fn new_room(&mut self, user: SocketAddr) -> u64 {
        if let Some(room_id) = self.user_exists(user) {
            room_id
        } else {
            let room = Room::new();
            let id = room.id;
            self.rooms.push(room);
            id
        }
    }

    pub fn new_one_room(&mut self) -> u64 {
        let room = Room::new_latest();
        let id = room.id;
        self.rooms.push(room);
        id
    }

    pub fn room_exists(&self, room_id: u64) -> bool {
        self.rooms.iter().any(|r| r.id.eq(&room_id))
    }

    pub fn user_exists(&self, user: SocketAddr) -> Option<u64> {
        self.rooms
            .iter()
            .find(|r| r.user_exists(user))
            .map(|r| r.id)
    }
}

static ROOM_ID: LazyLock<AtomicU64> = sync::LazyLock::new(|| AtomicU64::new(0));

fn fetch_latest_room_id() -> u64 {
    ROOM_ID.fetch_add(1, sync::atomic::Ordering::SeqCst)
}

#[derive(Debug, Clone)]
pub struct Room {
    id: u64,
    users: Vec<SocketAddr>, // ipAddr as the user idenitity
    msgs: Vec<Msg>,
    senders: Vec<mpsc::UnboundedSender<Msg>>,
}

impl Room {
    pub fn new() -> Self {
        Self {
            id: fetch_latest_room_id(),
            users: vec![],
            msgs: vec![],
            senders: vec![], // recver,
        }
    }

    pub fn new_latest() -> Self {
        Self {
            id: ROOM_ID.load(sync::atomic::Ordering::Relaxed),
            users: vec![],
            msgs: vec![],
            senders: vec![], // recver,
        }
    }

    pub fn add_user(&mut self, user: SocketAddr) {
        self.users.push(user);
    }

    pub fn remove_user(&mut self, user: &SocketAddr) {
        self.users.retain(|u| u.ne(user));
    }
    pub fn add_sender(&mut self, sender: UnboundedSender<Msg>) {
        self.senders.push(sender);
    }

    pub fn add_msg(&mut self, msg: Msg) {
        self.msgs.push(msg);
    }

    pub fn add_msgs(&mut self, msgs: &[Msg]) {
        self.msgs.append(&mut msgs.to_vec());
    }

    pub fn user_exists(&self, user: SocketAddr) -> bool {
        self.users.iter().any(|i| i.eq(&user))
    }

    // 清理已关闭的 senders
    fn cleanup_closed_senders(&mut self) {
        self.senders.retain(|s| !s.is_closed());
    }
}

#[derive(Debug, Clone)]
pub struct Msg {
    pub user: SocketAddr,
    pub data: String,
}

impl Msg {
    pub fn msg(&self) -> String {
        Msg::to_string(self.user, self.data.clone())
    }
    pub fn to_string(user: SocketAddr, data: String) -> String {
        format!("[{}]: {}\n", user, data)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".into(),
            port: 59414,
        }
    }
}

pub async fn run(config: Config) -> AnyResult<()> {
    let socket_addr_str = format!("{}:{}", config.ip, config.port);
    let socket_addr = SocketAddr::from_str(&socket_addr_str).unwrap();
    let tcp_listener = TcpListener::bind(socket_addr)
        .await
        .inspect_err(|e| eprintln!("Failed to bind on {}, {}", socket_addr, e))
        .unwrap();

    println!("tcp listen on {}", tcp_listener.local_addr().unwrap());

    let app_state = Arc::new(Mutex::new(AppState { rooms: vec![] }));

    loop {
        let (stream, user) = match tcp_listener.accept().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to accept: {}", e);
                break;
            }
        };

        let app_state = app_state.clone();
        tokio::spawn(async move {
            let (room_id, mut room_receiver) = {
                let state_lock = app_state.clone();
                let mut state = state_lock.lock().await;
                // let room_id = state.new_room(user);
                let room_id = state.new_one_room();
                let (room_sender, room_receiver) = mpsc::unbounded_channel::<Msg>();
                if let Some(room) = state.rooms.iter_mut().find(|r| r.id.eq(&room_id)) {
                    room.add_user(user);
                    room.add_sender(room_sender);
                }
                drop(state);

                (room_id, room_receiver)
            };

            let (mut s_rx, mut s_tx) = stream.into_split();
            let welcome = b"Welcome to sp chat room, some useful instruments: .create/.join [room_id]/.quic/.list";
            let _ = s_tx.write_all(welcome).await;

            // write task
            let write_task = tokio::spawn(async move {
                while let Some(msg) = room_receiver.recv().await {
                    if let Err(e) = s_tx.write_all(msg.msg().as_bytes()).await {
                        eprintln!("write err to {}: {}", user, e);
                        break;
                    }
                }
            });

            // read task
            let state_for_reader = Arc::clone(&app_state);
            let read_task = tokio::spawn(async move {
                let mut buf = [0u8; 2048];

                loop {
                    let len = match s_rx.read(&mut buf).await {
                        Ok(0) => {
                            println!("{user} closed");
                            break;
                        }
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("failed to read data from:{user} - {e}");
                            continue;
                        }
                    };

                    let data = String::from_utf8_lossy(&buf[..len]).into_owned();

                    let msg = Msg { user, data };

                    let senders = {
                        let mut state = state_for_reader.lock().await;
                        if let Some(room) = state.rooms.iter_mut().find(|r| r.id.eq(&room_id)) {
                            room.senders.clone()
                        } else {
                            vec![]
                        }
                    };

                    for sender in senders {
                        match sender.send(msg.clone()) {
                            Ok(()) => {}
                            Err(e) => {
                                // let _ = s_tx
                                //     .write_all(format!("Failed to send data to room: {e}").as_bytes())
                                //     .await;
                                eprintln!("Failed to send data to room: {e}");
                            }
                        }
                    }
                }

                // ON CONNECTION END: ensure we remove user & cleanup senders (short lock)
                {
                    let mut state = state_for_reader.lock().await;
                    if let Some(room) = state.rooms.iter_mut().find(|r| r.id == room_id) {
                        room.remove_user(&user);
                        room.cleanup_closed_senders();
                        // optionally remove empty room from state.rooms
                    }
                }
            });

            let _ = tokio::join!(read_task, write_task);
        });
    }

    Ok(())
}

type RoomID = u64;

#[derive(Debug, PartialEq)]
pub enum Action {
    Create,
    Join(RoomID),
    Quit,
    List,
}

impl FromStr for Action {
    type Err = AnyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parse_join_str = |s: &str| {
            if s.starts_with(".join") {
                let mut parts = s.splitn(2, ' ');
                parts.next();
                if let Some(num) = parts.next() {
                    return u64::from_str(num).map_err(|e| AnyError::wrap(e));
                }
            }

            Err(AnyError::quick(
                "No such action, available actions\n.create\n.join [room_id]\n.quic\n.list",
                anyverr::ErrKind::ValueValidation,
            ))
        };

        match s.to_lowercase().trim() {
            s if s == ".create" => Ok(Action::Create),
            s if s.starts_with(".join") => parse_join_str(s).map(|n| Action::Join(n)),
            s if s == ".quic" => Ok(Action::Quit),
            s if s == ".list" => Ok(Action::List),
            _ => Err(AnyError::quick(
                "No such action, available actions\n.create\n.join room_id\n.quic\n.list",
                anyverr::ErrKind::ValueValidation,
            )),
        }
    }
}
