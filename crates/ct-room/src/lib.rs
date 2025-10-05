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
    net::{TcpListener, TcpStream},
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

// ## 优化：使用 HashMap 存储房间，提高查找效率
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
    msgs: Vec<Msg>,
    senders: HashMap<SocketAddr, Arc<mpsc::UnboundedSender<Msg>>>,
}

impl Room {
    pub fn new() -> Self {
        Self {
            id: fetch_latest_room_id(),
            users: vec![],
            msgs: vec![],
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

    // 当一个用户的 sender 关闭时，同时从 users 列表和 senders 哈希图中移除
    fn cleanup_closed_senders(&mut self) {
        let mut closed_users = vec![];
        self.senders.retain(|user, sender| {
            if sender.is_closed() {
                closed_users.push(*user);
                false
            } else {
                true
            }
        });

        for user in closed_users {
            self.users.retain(|u| u != &user);
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

// ## 修复：主循环不再阻塞
pub async fn run(config: Config) -> AnyResult<()> {
    let socket_addr_str = format!("{}:{}", config.ip, config.port);
    let socket_addr = SocketAddr::from_str(&socket_addr_str).map_err(|e| AnyError::wrap(e))?;
    let tcp_listener = TcpListener::bind(socket_addr)
        .await
        .map_err(|e| AnyError::wrap(e))?;

    println!(
        "tcp listen on {}",
        tcp_listener.local_addr().map_err(|e| AnyError::wrap(e))?
    );

    let app_state = Arc::new(Mutex::new(AppState::default()));

    loop {
        let (stream, user) = match tcp_listener.accept().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to accept: {}", e);
                continue; // 不要让单个错误导致整个服务器崩溃
            }
        };

        let app_state = app_state.clone();
        // 为每个连接生成一个独立的任务，防止阻塞主循环
        tokio::spawn(async move {
            println!("New connection from: {}", user);
            if let Err(e) = handle_connection(stream, user, app_state).await {
                eprintln!("Error handling connection for {}: {}", user, e);
            }
        });
    }
}

// ## 新增：将单个连接的完整逻辑封装到此函数中
async fn handle_connection(
    stream: TcpStream,
    user: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
) -> AnyResult<()> {
    let (mut s_rx, mut s_tx) = io::split(stream);
    let welcome =
        b"Welcome to sp chat room, some useful instruments: .create/.join [room_id]/.quit/.list\n";
    s_tx.write_all(welcome)
        .await
        .map_err(|e| AnyError::wrap(e))?;

    let mut buf = [0u8; 128];
    let len = s_rx.read(&mut buf).await.map_err(|e| AnyError::wrap(e))?;
    if len == 0 {
        // 客户端直接断开连接
        return Ok(());
    }
    let input_str = String::from_utf8_lossy(&buf[..len]);
    let action = match Action::from_str(&input_str) {
        Ok(a) => a,
        Err(e) => {
            s_tx.write_all(e.to_string().as_bytes())
                .await
                .map_err(|e| AnyError::wrap(e))?;
            return Ok(());
        }
    };

    let room_state = match action {
        Action::Create => handle_create(app_state.clone(), user).await,
        Action::Join(_) => handle_join(&action, app_state.clone(), user).await,
        Action::Quit => handle_quit(app_state.clone(), user).await,
        Action::List => handle_list(app_state.clone()).await,
    };

    if let Some(msg) = room_state.message {
        s_tx.write_all(msg.as_bytes())
            .await
            .map_err(|e| AnyError::wrap(e))?;
    }

    // 对于 .list, .quit 等非聊天室指令，到此结束连接
    if room_state.room_id.is_none() || room_state.receiver.is_none() {
        return Ok(());
    }

    let room_id = room_state.room_id.unwrap();
    let mut room_receiver = room_state.receiver.unwrap();
    let room_sender = room_state.sender.unwrap();

    // -- 进入聊天循环 --

    // 写任务：从 channel 接收消息并发送给客户端
    let write_task = tokio::spawn(async move {
        while let Some(msg) = room_receiver.recv().await {
            // 不把自己发的消息再发回给自己
            // if msg.user != user {
            if s_tx.write_all(msg.msg().as_bytes()).await.is_err() {
                break;
            }
            // }
        }
    });

    // 读任务：从客户端读取消息并广播到 channel
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
                // 使用 .get() 高效查找
                state
                    .rooms
                    .get(&room_id)
                    .map_or(HashMap::new(), |r| r.senders.clone())
            };

            for sender in senders.values() {
                // 如果发送失败，说明对方可能已掉线，忽略错误
                let _ = sender.send(msg.clone());
            }
        }

        // 连接结束时，清理用户资源
        let mut state = app_state.lock().await;
        if let Some(room) = state.rooms.get_mut(&room_id) {
            println!("{} disconnected from room {}", user, room_id);
            room.remove_user(&user);
            // 可以在这里广播用户离开的消息
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
    // 构造器方法保持不变...
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

// ## 优化：Action Handler 使用 HashMap API
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

        if let Some(room) = state.rooms.get_mut(room_id) {
            if room.user_exists(user) {
                return RoomState::empty()
                    .with_message(Some("You are already in the room.\n".to_string()));
            }

            let (room_sender, room_receiver) = mpsc::unbounded_channel::<Msg>();
            let arc_room_sender = Arc::new(room_sender);

            let join_msg = Msg {
                user,
                data: "has joined the room.".to_string(),
            };
            for sender in room.senders.values() {
                let _ = sender.send(join_msg.clone());
            }

            room.add_user(&user);
            room.update_sender(&user, arc_room_sender.clone());

            let success_msg = format!("Successfully joined room: {}\n", room_id);
            return RoomState::new(Some(*room_id))
                .with_receiver(Some(room_receiver))
                .with_sender(Some(arc_room_sender))
                .with_message(Some(success_msg));
        } else {
            return RoomState::empty().with_message(Some("Room not found.\n".to_string()));
        }
    }
    RoomState::empty()
}

async fn handle_create(state: Arc<Mutex<AppState>>, user: SocketAddr) -> RoomState {
    let mut state = state.lock().await;
    let room_id = state.new_room();

    let (room_sender, room_receiver) = mpsc::unbounded_channel::<Msg>();
    let arc_room_sender = Arc::new(room_sender);

    // 刚创建的 room 一定存在，可以直接 unwrap
    let room = state.rooms.get_mut(&room_id).unwrap();
    room.add_user(&user);
    room.update_sender(&user, arc_room_sender.clone());

    let msg = format!("Successfully created room: {}\n", room_id);
    RoomState::new(Some(room_id))
        .with_receiver(Some(room_receiver))
        .with_sender(Some(arc_room_sender))
        .with_message(Some(msg))
}

// ## 修复：`handle_quit` 逻辑
async fn handle_quit(state: Arc<Mutex<AppState>>, user: SocketAddr) -> RoomState {
    let mut state = state.lock().await;

    if let Some(room_id) = state.user_exists(user) {
        if let Some(room) = state.rooms.get_mut(&room_id) {
            room.remove_user(&user);
            let msg = format!("Successfully quit room: {}\n", room_id);
            return RoomState::new(Some(room_id)).with_message(Some(msg));
        }
    }

    RoomState::empty().with_message(Some("You are not in any room.\n".to_string()))
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
    // from_str 逻辑保持不变...
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
