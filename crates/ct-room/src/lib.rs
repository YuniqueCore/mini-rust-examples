use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{self, Arc, LazyLock, atomic::AtomicU64},
};

use anyverr::{AnyError, AnyResult};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{
        Mutex,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
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
    pub fn new_room(&mut self) -> u64 {
        let room = Room::new();
        let id = room.id;
        self.rooms.push(room);
        id
    }
    pub fn last_room(&mut self, user: SocketAddr) -> u64 {
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
    senders: HashMap<SocketAddr, Arc<mpsc::UnboundedSender<Msg>>>,
}

impl Room {
    pub fn new() -> Self {
        Self {
            id: fetch_latest_room_id(),
            users: vec![],
            msgs: vec![],
            senders: HashMap::new(), // recver,
        }
    }

    pub fn new_latest() -> Self {
        Self {
            id: ROOM_ID.load(sync::atomic::Ordering::Relaxed),
            users: vec![],
            msgs: vec![],
            senders: HashMap::new(), // recver,
        }
    }

    pub fn add_user(&mut self, user: &SocketAddr) {
        self.users.push(user.clone());
    }

    pub fn remove_user(&mut self, user: &SocketAddr) {
        self.users.retain(|u| u.ne(user));
        self.senders.remove(user);
    }

    pub fn update_sender(&mut self, user: &SocketAddr, sender: Arc<UnboundedSender<Msg>>) {
        self.senders.insert(user.clone(), sender);
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
        let mut users = vec![];
        self.senders.retain(|user, sender| {
            if sender.is_closed() {
                users.push(user.clone());
                false
            } else {
                true
            }
        });

        for user in users {
            self.remove_user(&user);
        }
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

        let (mut s_rx, mut s_tx) = stream.into_split();
        let welcome = b"Welcome to sp chat room, some useful instruments: .create/.join [room_id]/.quic/.list";
        let _ = s_tx.write_all(welcome).await;

        let mut buf = [0u8; 128];
        let len = s_rx.read(&mut buf).await.unwrap();
        let input_str = String::from_utf8_lossy(&buf[..len]);
        let action = match Action::from_str(&input_str) {
            Ok(a) => a,
            Err(e) => {
                let _ = s_tx.write_all(e.to_string().as_bytes()).await;
                continue;
            }
        };

        let app_state_for_action = app_state.clone();
        let room_state = match action {
            Action::Create => handle_create(&action, app_state_for_action, user).await,
            Action::Join(_) => handle_join(&action, app_state_for_action, user).await,
            Action::Quit => handle_quit(&action, app_state_for_action, user).await,
            Action::List => handle_list(&action, app_state_for_action, user).await,
        };

        if let Some(msg) = room_state.message {
            let _ = s_tx.write_all(msg.as_bytes()).await;
        }

        if room_state.room_id.is_none() || room_state.receiver.is_none() {
            continue;
        }

        let room_id = room_state.room_id.unwrap();
        let mut room_receiver = room_state.receiver.unwrap();
        let room_sender = room_state.sender.unwrap();

        let app_state_for_reader = app_state.clone();
        tokio::spawn(async move {
            // write task
            let write_task = tokio::spawn(async move {
                while let Some(msg) = room_receiver.recv().await {
                    if let Err(e) = s_tx.write_all(msg.msg().as_bytes()).await {
                        eprintln!("write err to {} in room: {}: {}", user, room_id, e);
                        break;
                    }
                }
            });

            let read_task = tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                loop {
                    let len = match s_rx.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) => {
                            if let Err(e) = room_sender.send(Msg {
                                user,
                                data: e.to_string(),
                            }) {
                                eprintln!("error on reading: {e}");
                            };
                            break;
                        }
                    };

                    let data = String::from_utf8_lossy(&buf[..len]).into_owned();
                    let msg = Msg { user, data };
                    let senders = {
                        let mut state = app_state_for_reader.lock().await;
                        if let Some(room) = state.rooms.iter_mut().find(|r| r.id.eq(&room_id)) {
                            room.senders.clone()
                        } else {
                            HashMap::new()
                        }
                    };
                    for (_user, sender) in senders {
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

                //  ON CONNECTION END: ensure we remove user & cleanup senders (short lock)
                {
                    let mut state = app_state_for_reader.lock().await;
                    if let Some(room) = state.rooms.iter_mut().find(|r| r.id == room_id) {
                        room.remove_user(&user);
                        room.cleanup_closed_senders();
                    }
                }
            });

            let _ = tokio::join!(write_task, read_task);
        });
    }

    Ok(())
}

#[derive(Debug)]
struct RoomState {
    room_id: Option<u64>,
    receiver: Option<UnboundedReceiver<Msg>>,
    sender: Option<Arc<UnboundedSender<Msg>>>,
    message: Option<String>,
}

impl RoomState {
    pub fn empty() -> Self {
        Self {
            room_id: None,
            receiver: None,
            sender: None,
            message: None,
        }
    }
    pub fn new(room_id: Option<u64>) -> Self {
        Self {
            room_id,
            receiver: None,
            sender: None,
            message: None,
        }
    }

    pub fn with_receiver(mut self, receiver: Option<UnboundedReceiver<Msg>>) -> Self {
        self.receiver = receiver;
        self
    }

    pub fn with_sender(mut self, sender: Option<Arc<UnboundedSender<Msg>>>) -> Self {
        self.sender = sender;
        self
    }

    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.message = message;
        self
    }
}

async fn handle_list(
    action: &Action,
    app_state: Arc<Mutex<AppState>>,
    _user: SocketAddr,
) -> RoomState {
    let state = app_state.lock().await;
    if *action == Action::List {
        let rooms_id = state
            .rooms
            .iter()
            .map(|r| r.id.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        return RoomState::new(None).with_message(Some(rooms_id));
    }

    RoomState::empty()
}

async fn handle_join(
    action: &Action,
    app_state: Arc<Mutex<AppState>>,
    user: SocketAddr,
) -> RoomState {
    if let Action::Join(room_id) = action {
        let mut state = app_state.lock().await;
        let mut msg = String::new();
        if state.room_exists(*room_id) {
            let room = state.rooms.iter_mut().find(|r| r.id.eq(room_id));
            if room.is_none() {
                msg.push_str("No such room\n");
            }
            let room = room.unwrap();
            if room.user_exists(user) {
                msg.push_str("You have already in the room");
            }
            let (room_sender, room_receiver) = mpsc::unbounded_channel::<Msg>();
            let arc_room_sender = Arc::new(room_sender);
            room.add_user(&user);
            room.update_sender(&user, arc_room_sender.clone());

            return RoomState::new(Some(*room_id))
                .with_receiver(Some(room_receiver))
                .with_sender(Some(arc_room_sender))
                .with_message(Some(msg));
        }
    }

    RoomState::empty()
}

async fn handle_create(
    action: &Action,
    state: Arc<Mutex<AppState>>,
    user: SocketAddr,
) -> RoomState {
    let state_lock = state.lock().await;
    let mut state = state_lock;
    if *action == Action::Create {
        let room_id = state.new_room();
        let (room_sender, room_receiver) = mpsc::unbounded_channel::<Msg>();
        let arc_room_sender = Arc::new(room_sender);
        if let Some(room) = state.rooms.iter_mut().find(|r| r.id.eq(&room_id)) {
            room.add_user(&user);
            room.update_sender(&user, arc_room_sender.clone());
        }
        drop(state);

        RoomState::new(Some(room_id))
            .with_receiver(Some(room_receiver))
            .with_sender(Some(arc_room_sender))
            .with_message(Some(format!("successfully create the room: {room_id}")))
    } else {
        RoomState::empty()
    }
}

async fn handle_quit(action: &Action, state: Arc<Mutex<AppState>>, user: SocketAddr) -> RoomState {
    let state_lock = state.lock().await;
    let mut state = state_lock;
    if *action == Action::Quit {
        let mut room_id = 0;
        if let Some(inner_room_id) = state.user_exists(user) {
            if let Some(room) = state.rooms.iter_mut().find(|r| r.id.eq(&inner_room_id)) {
                room.remove_user(&user);
                room_id = inner_room_id;
            }
        }
        drop(state);

        RoomState::new(Some(room_id))
            .with_message(Some(format!("successfully quit the room: {room_id}")))
    } else {
        RoomState::empty()
    }
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
