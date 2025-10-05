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

// Config, AppState, Room, Msg 等结构体和 impl 保持不变
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    ip: String,
    port: u16,
}

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
    let socket_addr = SocketAddr::from_str(&socket_addr_str).map_err(AnyError::wrap)?;
    let tcp_listener = TcpListener::bind(socket_addr)
        .await
        .map_err(AnyError::wrap)?;

    println!(
        "tcp listen on {}",
        tcp_listener.local_addr().map_err(AnyError::wrap)?
    );

    let app_state = Arc::new(Mutex::new(AppState::default()));

    loop {
        let (stream, user) = match tcp_listener.accept().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to accept: {}", e);
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

// ## 重构：handle_connection 现在是“大厅”，负责循环处理指令
async fn handle_connection(
    stream: TcpStream,
    user: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
) -> AnyResult<()> {
    let (mut s_rx, mut s_tx) = io::split(stream);
    let welcome = b"Welcome! You are in the lobby.\nCommands: .create, .join [id], .list, .quit\n";
    s_tx.write_all(welcome).await.map_err(AnyError::wrap)?;

    // 大厅循环 (Lobby Loop)
    loop {
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
                continue; // 继续等待下一个指令
            }
        };

        // 用户执行 .quit 指令，直接退出
        if action == Action::Quit {
            handle_quit(app_state.clone(), user).await;
            s_tx.write_all(b"Goodbye!\n")
                .await
                .map_err(AnyError::wrap)?;
            return Ok(());
        }

        let room_state = match action {
            Action::Create => handle_create(app_state.clone(), user).await,
            Action::Join(_) => handle_join(&action, app_state.clone(), user).await,
            Action::List => handle_list(app_state.clone()).await,
            Action::Quit => unreachable!(), // 上面已经处理过
        };

        if let Some(msg) = room_state.message {
            s_tx.write_all(msg.as_bytes())
                .await
                .map_err(AnyError::wrap)?;
        }

        // 如果指令是 .create 或 .join 且成功，则 room_state 会包含必要信息
        // 此时跳出大厅循环，进入聊天会话
        if let (Some(id), Some(rx), Some(tx)) =
            (room_state.room_id, room_state.receiver, room_state.sender)
        {
            return run_chat_session(s_rx, s_tx, user, app_state, id, rx, tx).await;
        }
        // 对于 .list 等指令，则继续在大厅循环中等待下一个指令
    }
}

// 将聊天会话逻辑独立成一个函数
async fn run_chat_session(
    mut s_rx: ReadHalf<TcpStream>,
    mut s_tx: WriteHalf<TcpStream>,
    user: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
    room_id: u64,
    mut room_receiver: UnboundedReceiver<Msg>,
    room_sender: Arc<UnboundedSender<Msg>>,
) -> AnyResult<()> {
    // 写任务
    let write_task = tokio::spawn(async move {
        while let Some(msg) = room_receiver.recv().await {
            if s_tx.write_all(msg.msg().as_bytes()).await.is_err() {
                break;
            }
        }
    });

    // 读任务
    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            let len = match s_rx.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    let _ = room_sender.send(Msg {
                        user,
                        data: e.to_string(),
                    });
                    break;
                }
            };

            let data = String::from_utf8_lossy(&buf[..len]).into_owned();
            let msg = Msg { user, data };

            let senders = {
                let state = app_state.lock().await;
                state
                    .rooms
                    .get(&room_id)
                    .map_or(HashMap::new(), |r| r.senders.clone())
            };

            for sender in senders.values() {
                let _ = sender.send(msg.clone());
            }
        }

        // 连接结束时，清理用户资源
        let mut state = app_state.lock().await;
        if let Some(room) = state.rooms.get_mut(&room_id) {
            println!("{} disconnected from room {}", user, room_id);
            room.remove_user(&user);
            let leave_msg = Msg {
                user,
                data: "has left the room.".to_string(),
            };
            for sender in room.senders.values() {
                let _ = sender.send(leave_msg.clone());
            }
        }
    });

    let _ = tokio::join!(write_task, read_task);
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

// handle_* 系列函数基本保持不变，只是做了微调
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
        let mut app_state = app_state.lock().await;

        if app_state.user_exists(user).is_some() {
            return RoomState::empty().with_message(Some(
                "You are already in a room. Use .quit to leave first.\n".to_string(),
            ));
        }

        if let Some(room) = app_state.rooms.get_mut(room_id) {
            let (tx, rx) = mpsc::unbounded_channel::<Msg>();
            let arc_tx = Arc::new(tx);

            let join_msg = Msg {
                user,
                data: "has joined the room.".to_string(),
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

async fn handle_create(app_state: Arc<Mutex<AppState>>, user: SocketAddr) -> RoomState {
    let mut state = app_state.lock().await;

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

async fn handle_quit(app_state: Arc<Mutex<AppState>>, user: SocketAddr) {
    let mut state = app_state.lock().await;
    if let Some(room_id) = state.user_exists(user) {
        if let Some(room) = state.rooms.get_mut(&room_id) {
            room.remove_user(&user);
            println!("User {} quit from room {}", user, room_id);
        }
    }
}

// Action enum 和 FromStr impl 保持不变
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

        match s.to_lowercase().trim() {
            s if s == ".create" => Ok(Action::Create),
            s if s.starts_with(".join") => parse_join_str(s).map(Action::Join),
            s if s == ".quit" => Ok(Action::Quit),
            s if s == ".list" => Ok(Action::List),
            _ => Err(AnyError::quick(
                "No such action, available actions:\n.create\n.join [room_id]\n.quit\n.list",
                anyverr::ErrKind::ValueValidation,
            )),
        }
    }
}
