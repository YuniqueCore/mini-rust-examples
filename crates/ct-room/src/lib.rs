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

// 配置结构体
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

// --- 核心状态与数据结构 (无变动) ---

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

// 指令解析

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
            parts.next();
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

/// 连接状态
enum State {
    Lobby,
    Chatting(ChatSession),
    Shutdown,
}

/// 聊天会话所需的数据
struct ChatSession {
    room_id: u64,
    receiver: UnboundedReceiver<Msg>,
}

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

/// 连接处理器，现在是一个状态机驱动器
async fn handle_connection(
    stream: TcpStream,
    user: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
) -> AnyResult<()> {
    let (mut s_rx, mut s_tx) = io::split(stream);
    s_tx.write_all(
        b"Welcome! You are in the lobby.\nCommands: .create, .join [id], .list, .quit\n",
    )
    .await
    .map_err(|e| AnyError::wrap(e))?;

    // 初始状态为 Lobby
    let mut state = State::Lobby;

    loop {
        state = match state {
            State::Lobby => {
                handle_lobby_state(&mut s_rx, &mut s_tx, user, app_state.clone()).await?
            }
            State::Chatting(session) => {
                handle_chatting_state(session, &mut s_rx, &mut s_tx, user, app_state.clone())
                    .await?
            }
            State::Shutdown => {
                // 如果任何状态处理器要求关机，则跳出循环
                break;
            }
        };
    }

    Ok(())
}

/// 处理用户在大厅时的逻辑
async fn handle_lobby_state(
    s_rx: &mut ReadHalf<TcpStream>,
    s_tx: &mut WriteHalf<TcpStream>,
    user: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
) -> AnyResult<State> {
    let mut buf = [0u8; 128];
    let len = match s_rx.read(&mut buf).await {
        Ok(0) => return Ok(State::Shutdown), // 客户端断开
        Ok(n) => n,
        Err(e) => return Err(AnyError::wrap(e)),
    };

    let input_str = String::from_utf8_lossy(&buf[..len]);
    let action = match Action::from_str(&input_str) {
        Ok(a) => a,
        Err(e) => {
            s_tx.write_all(format!("Invalid command: {}\n", e).as_bytes())
                .await
                .map_err(|e| AnyError::wrap(e))?;
            return Ok(State::Lobby); // 保持在大厅状态
        }
    };

    if action == Action::Quit {
        s_tx.write_all(b"Goodbye!\n")
            .await
            .map_err(|e| AnyError::wrap(e))?;
        return Ok(State::Shutdown); // 转换到关机状态
    }

    let room_state = match action {
        Action::Create => handle_create(app_state, user).await,
        Action::Join(_) => handle_join(&action, app_state, user).await,
        Action::List => handle_list(app_state).await,
        Action::Quit => unreachable!(),
    };

    if let Some(msg) = room_state.message {
        s_tx.write_all(msg.as_bytes())
            .await
            .map_err(|e| AnyError::wrap(e))?;
    }

    if let (Some(room_id), Some(receiver)) = (room_state.room_id, room_state.receiver) {
        // 成功加入房间，转换到 Chatting 状态
        Ok(State::Chatting(ChatSession { room_id, receiver }))
    } else {
        // 否则，继续停留在 Lobby 状态
        Ok(State::Lobby)
    }
}

/// 处理用户在聊天室时的逻辑
async fn handle_chatting_state(
    mut session: ChatSession,
    s_rx: &mut ReadHalf<TcpStream>,
    s_tx: &mut WriteHalf<TcpStream>,
    user: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
) -> AnyResult<State> {
    let mut read_buf = [0u8; 2048];

    tokio::select! {
        // 监听来自房间其他用户的消息
        Some(msg) = session.receiver.recv() => {
            if msg.user != user {
                if s_tx.write_all(msg.msg().as_bytes()).await.is_err() {
                    handle_quit(app_state, user, session.room_id).await;
                    return Ok(State::Shutdown); // 写入失败，关闭连接
                }
            }
            Ok(State::Chatting(session)) // 保持在聊天状态
        }

        // 监听当前用户的键盘输入
        result = s_rx.read(&mut read_buf) => {
            match result {
                Ok(0) | Err(_) => { // 客户端断开
                    handle_quit(app_state, user, session.room_id).await;
                    Ok(State::Shutdown)
                },
                Ok(len) => {
                    let data = String::from_utf8_lossy(&read_buf[..len]);
                    if data.trim() == ".quit" {
                        handle_quit(app_state, user, session.room_id).await;
                        s_tx.write_all(b"You have left the room. Returning to lobby.\n").await.map_err(|e| AnyError::wrap(e))?;
                     return   Ok(State::Lobby); // 转换回大厅状态
                    }

                    let msg = Msg { user, data: data.into_owned() };
                    let senders = {
                        let state = app_state.lock().await;
                        state.rooms.get(&session.room_id).map_or(HashMap::new(), |r| r.senders.clone())
                    };
                    for s in senders.values() {
                        let _ = s.send(msg.clone());
                    }
                    Ok(State::Chatting(session)) // 保持在聊天状态
                }
            }
        }
    }
}

// Action Handlers

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

async fn handle_quit(app_state: Arc<Mutex<AppState>>, user: SocketAddr, room_id: u64) {
    let mut state = app_state.lock().await;
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
            if room.users.is_empty() {
                println!("Room {} is empty, removing it.", room_id);
                state.rooms.remove(&room_id);
            }
        }
    }
}
