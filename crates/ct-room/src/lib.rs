use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{self, Arc, LazyLock, atomic::AtomicU64},
};

use anyverr::{AnyError, AnyResult};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::{TcpListener, TcpStream},
    sync::{
        Mutex,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
    },
};

// --- 配置结构体 ---
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    ip: String,
    port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".into(),
            port: 59414,
        }
    }
}

// --- 核心状态与数据结构 ---

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub rooms: HashMap<u64, Room>,
}

impl AppState {
    pub fn new_room(&mut self) -> u64 {
        let room = Room::new();
        let id = room.id;
        self.rooms.insert(id, room);
        id
    }

    pub fn room_exists(&self, room_id: u64) -> bool {
        self.rooms.contains_key(&room_id)
    }

    pub fn user_exists(&self, user: SocketAddr) -> Option<u64> {
        self.rooms
            .values()
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
    users: Vec<SocketAddr>,
    senders: HashMap<SocketAddr, Arc<mpsc::UnboundedSender<Msg>>>,
}

impl Room {
    pub fn new() -> Self {
        Self {
            id: fetch_latest_room_id(),
            users: vec![],
            senders: HashMap::new(),
        }
    }

    pub fn add_user(&mut self, user: &SocketAddr) {
        if !self.users.contains(user) {
            self.users.push(*user);
        }
    }

    pub fn remove_user(&mut self, user: &SocketAddr) {
        self.users.retain(|u| u != user);
        self.senders.remove(user);
    }

    pub fn update_sender(&mut self, user: &SocketAddr, sender: Arc<UnboundedSender<Msg>>) {
        self.senders.insert(*user, sender);
    }

    pub fn user_exists(&self, user: SocketAddr) -> bool {
        self.users.iter().any(|i| i == &user)
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
        format!("[{}]: {}", user, data)
    }
}

// --- 指令解析 ---

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
        let s = s.trim();
        let parse_join_str = |s: &str| {
            let mut parts = s.trim().splitn(2, ' ');
            parts.next(); // skip ".join"
            if let Some(num_str) = parts.next() {
                u64::from_str(num_str.trim()).map_err(AnyError::wrap)
            } else {
                Err(AnyError::quick(
                    "Join command requires a room ID.",
                    anyverr::ErrKind::ValueValidation,
                ))
            }
        };

        match s.to_lowercase() {
            s if s == ".create" => Ok(Action::Create),
            s if s.starts_with(".join") => parse_join_str(&s).map(Action::Join),
            s if s == ".quit" => Ok(Action::Quit),
            s if s == ".list" => Ok(Action::List),
            _ => Err(AnyError::quick(
                "Not a command. Commands start with '.'",
                anyverr::ErrKind::ValueValidation,
            )),
        }
    }
}

// --- 服务器主逻辑 ---
pub async fn run(config: Config) -> AnyResult<()> {
    let socket_addr_str = format!("{}:{}", config.ip, config.port);
    let socket_addr = SocketAddr::from_str(&socket_addr_str).map_err(AnyError::wrap)?;
    let tcp_listener = TcpListener::bind(socket_addr)
        .await
        .map_err(AnyError::wrap)?;

    println!(
        "TCP server listening on {}",
        tcp_listener.local_addr().map_err(AnyError::wrap)?
    );

    let app_state = Arc::new(Mutex::new(AppState::default()));

    loop {
        let (stream, user) = match tcp_listener.accept().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        };

        let app_state = app_state.clone();
        tokio::spawn(async move {
            println!("New connection from: {}", user);
            if let Err(e) = handle_connection(stream, user, app_state).await {
                eprintln!("Error handling connection for {}: {}", user, e);
            }
            println!("Connection closed for: {}", user);
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    user: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
) -> AnyResult<()> {
    let (mut s_rx, mut s_tx) = io::split(stream);
    let welcome = b"Welcome! You are in the lobby.\nCommands: .create, .join [id], .list, .quit\n";
    s_tx.write_all(welcome).await.map_err(AnyError::wrap)?;

    // 外层循环：大厅 (Lobby)
    'lobby: loop {
        let mut buf = [0u8; 128];
        let len = match s_rx.read(&mut buf).await {
            Ok(0) => return Ok(()), // 客户端断开
            Ok(n) => n,
            Err(e) => return Err(AnyError::wrap(e)),
        };

        let input_str = String::from_utf8_lossy(&buf[..len]);
        let action = match Action::from_str(&input_str) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("Invalid command: {}\n", e);
                s_tx.write_all(msg.as_bytes())
                    .await
                    .map_err(AnyError::wrap)?;
                continue;
            }
        };

        if action == Action::Quit {
            s_tx.write_all(b"Goodbye!\n")
                .await
                .map_err(AnyError::wrap)?;
            return Ok(());
        }

        let room_state = match action {
            Action::Create => handle_create(app_state.clone(), user).await,
            Action::Join(_) => handle_join(&action, app_state.clone(), user).await,
            Action::List => handle_list(app_state.clone()).await,
            Action::Quit => unreachable!(),
        };

        if let Some(msg) = room_state.message {
            s_tx.write_all(msg.as_bytes())
                .await
                .map_err(AnyError::wrap)?;
        }

        // 如果成功加入房间，则进入内层聊天循环
        if let (Some(room_id), Some(mut receiver), Some(_sender)) =
            (room_state.room_id, room_state.receiver, room_state.sender)
        {
            // 内层循环：聊天室
            loop {
                let mut read_buf = [0u8; 2048];
                tokio::select! {
                    // 监听来自房间其他用户的消息
                    Some(msg) = receiver.recv() => {
                        if msg.user != user {
                            // 不显示自己发的消息
                            if s_tx.write_all(msg.msg().as_bytes()).await.is_err() {
                                break; // 写入失败，结束会话
                            }
                        }
                    }

                    // 监听当前用户的键盘输入
                    result = s_rx.read(&mut read_buf) => {
                        let len = match result {
                            Ok(0) | Err(_) => break, // 客户端断开
                            Ok(n) => n,
                        };

                        let data = String::from_utf8_lossy(&read_buf[..len]).into_owned();

                        if let Ok(action) = Action::from_str(&data) {
                            if action == Action::Quit{
                                handle_quit(app_state.clone(), user, room_id).await;
                                s_tx.write_all(b"You have left the room. Returning to lobby.\n").await.map_err(AnyError::wrap)?;
                                continue 'lobby; // 返回到大厅循环
                            }
                        }

                        // 作为聊天消息广播
                        let msg = Msg { user, data };
                        let senders = {
                             let state = app_state.lock().await;
                             state.rooms.get(&room_id).map_or(HashMap::new(), |r| r.senders.clone())
                        };
                        for s in senders.values() {
                            let _ = s.send(msg.clone());
                        }
                    }
                }
            } // 内层循环结束

            // 当从聊天循环 break (如客户端断开) 时，确保清理资源
            handle_quit(app_state.clone(), user, room_id).await;
        }
    }
}

// --- Action Handlers ---

#[derive(Debug)]
struct RoomState {
    room_id: Option<u64>,
    receiver: Option<UnboundedReceiver<Msg>>,
    sender: Option<Arc<UnboundedSender<Msg>>>,
    message: Option<String>,
}

impl RoomState {
    pub fn empty() -> Self {
        /* ... */
        Self {
            room_id: None,
            receiver: None,
            sender: None,
            message: None,
        }
    }
    pub fn new(room_id: Option<u64>) -> Self {
        /* ... */
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

async fn handle_list(app_state: Arc<Mutex<AppState>>) -> RoomState {
    let state = app_state.lock().await;
    let rooms_id = state
        .rooms
        .keys()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let msg = if rooms_id.is_empty() {
        "No active rooms.\n".to_string()
    } else {
        format!("Active rooms: [ {} ]\n", rooms_id)
    };

    RoomState::empty().with_message(Some(msg))
}

async fn handle_join(
    action: &Action,
    app_state: Arc<Mutex<AppState>>,
    user: SocketAddr,
) -> RoomState {
    if let Action::Join(room_id) = action {
        let mut state = app_state.lock().await;

        if state.user_exists(user).is_some() {
            return RoomState::empty().with_message(Some(
                "You are already in a room. Use .quit to leave first.\n".to_string(),
            ));
        }

        if let Some(room) = state.rooms.get_mut(room_id) {
            let (tx, rx) = mpsc::unbounded_channel::<Msg>();
            let arc_tx = Arc::new(tx);

            let join_msg = Msg {
                user,
                data: "has joined the room.\n".to_string(),
            };
            for sender in room.senders.values() {
                let _ = sender.send(join_msg.clone());
            }

            room.add_user(&user);
            room.update_sender(&user, arc_tx.clone());

            let success_msg = format!(
                "Successfully joined room: {}. You can start chatting.\n",
                room_id
            );
            return RoomState::new(Some(*room_id))
                .with_receiver(Some(rx))
                .with_sender(Some(arc_tx))
                .with_message(Some(success_msg));
        } else {
            return RoomState::empty().with_message(Some("Room not found.\n".to_string()));
        }
    }
    RoomState::empty()
}

async fn handle_create(state: Arc<Mutex<AppState>>, user: SocketAddr) -> RoomState {
    let mut state = state.lock().await;

    if state.user_exists(user).is_some() {
        return RoomState::empty().with_message(Some(
            "You are already in a room. Use .quit to leave first.\n".to_string(),
        ));
    }

    let room_id = state.new_room();
    let (tx, rx) = mpsc::unbounded_channel::<Msg>();
    let arc_tx = Arc::new(tx);

    let room = state.rooms.get_mut(&room_id).unwrap();
    room.add_user(&user);
    room.update_sender(&user, arc_tx.clone());

    let msg = format!(
        "Successfully created and joined room: {}. You can start chatting.\n",
        room_id
    );
    RoomState::new(Some(room_id))
        .with_receiver(Some(rx))
        .with_sender(Some(arc_tx))
        .with_message(Some(msg))
}

async fn handle_quit(state: Arc<Mutex<AppState>>, user: SocketAddr, room_id: u64) {
    let mut state = state.lock().await;
    if let Some(room) = state.rooms.get_mut(&room_id) {
        if room.user_exists(user) {
            println!("User {} quit from room {}", user, room_id);
            room.remove_user(&user);
            let leave_msg = Msg {
                user,
                data: "has left the room.\n".to_string(),
            };
            for sender in room.senders.values() {
                let _ = sender.send(leave_msg.clone());
            }
        }
    }
}
