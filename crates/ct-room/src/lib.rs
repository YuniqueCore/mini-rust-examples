use std::{
    net::SocketAddr,
    str::FromStr,
    sync::{self, Arc, LazyLock, Mutex, atomic::AtomicU64},
};

use anyverr::{AnyError, AnyResult};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{broadcast, mpsc},
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
    // sender:Arc<mpsc::UnboundedSender<Msg>>,
    // recver: mpsc::UnboundedReceiver<Msg>,
}

impl Room {
    pub fn new() -> Self {
        // let (sender, recver) = mpsc::unbounded_channel::<Msg>();
        Self {
            id: fetch_latest_room_id(),
            users: vec![],
            msgs: vec![],
            // sender: Arc::new(sender),
            // recver,
        }
    }

    pub fn add_user(&mut self, user: SocketAddr) {
        self.users.push(user);
    }

    pub fn add_msg(&mut self, msg: Msg) {
        self.msgs.push(msg);
    }

    pub fn user_exists(&self, user: SocketAddr) -> bool {
        self.users.iter().any(|i| i.eq(&user))
    }
}

#[derive(Debug, Clone)]
pub struct Msg {
    pub user: SocketAddr,
    pub data: String,
}

impl Msg {
    pub fn to_string(user: SocketAddr, data: String) -> String {
        format!("[{}]: {}", user, data)
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
        let (mut stream, user) = match tcp_listener.accept().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to accept: {}", e);
                break;
            }
        };
        let (room_sender, mut room_receiver) = mpsc::unbounded_channel::<Msg>();
        let state_lock = app_state.clone();
        let mut state = state_lock.lock().expect("should be locked");
        let room_id = state.new_room(user);

        let (s_rx, s_tx) = stream.split();

        let mut buf = [0u8; 2048];
        loop {
            let mut room_buf = Vec::with_capacity(100);
            const ROOM_CAPABILITY: usize = 5;
            let msg_count = room_receiver
                .recv_many(&mut room_buf, ROOM_CAPABILITY)
                .await;
        }

        handle_user_msg(user, room_sender, s_rx, s_tx, buf).await;
    }

    Ok(())
}

async fn handle_user_msg(
    user: SocketAddr,
    room_sender: mpsc::UnboundedSender<Msg>,
    mut s_rx: tokio::net::tcp::ReadHalf<'_>,
    mut s_tx: tokio::net::tcp::WriteHalf<'_>,
    mut buf: [u8; 2048],
) {
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
        match room_sender.send(msg) {
            Ok(()) => {}
            Err(e) => {
                let _ = s_tx
                    .write_all(format!("Failed to send data to room: {e}").as_bytes())
                    .await;
            }
        }
    }
}
