mod obj;
mod inflater;

use inflater::Inflater;
use obj::{Data, Payload, Value, Reaction};
use tokio::net::TcpStream;
use tokio_tungstenite as tg;
use tg::{MaybeTlsStream, WebSocketStream, tungstenite::Message as WSMessage,};
use futures_util::{SinkExt, StreamExt};
use eetf;
use json::{self, object};
use std::{io::Cursor, sync::mpsc::{self}, collections::HashMap};
use bounded_vec_deque::BoundedVecDeque;

use hyper::{Body, Client, Request, client::{Builder, HttpConnector}};
use hyper_rustls::{self, HttpsConnector};

use crate::obj::{Message, User};



// Client version information, these should change alot and must be changed here to avoid suspicion
const CLIENT_VERSION: &'static str = "1.0.46";
const CLIENT_BUILD_NUMBER: &'static i32 = &126462;
const MESSAGE_CACHE_PER_CHANNEL: &'static usize = &1000;


fn headtail(s: &String) -> (&str, Option<&str>) {
    let mut split = s.splitn(2, ' ');
    let head = split.next().unwrap();
    let tail = split.next();
    (head, tail)
}


async fn create_socket() -> (WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, hyper::Response<()>) {
    let request = Request::builder()
    .uri("wss://gateway.discord.gg/?encoding=etf&v=6&compress=zlib-stream")
    .header("Upgrade", "websocket")
    .body(())
    .unwrap();

    let config = tg::tungstenite::protocol::WebSocketConfig {
        max_frame_size: None,
        ..tg::tungstenite::protocol::WebSocketConfig::default()
    };

    tg::connect_async_with_config(request, Some(config)).await.unwrap()
}
#[tokio::main]
async fn main() {

    let token = match std::fs::read_to_string("TOKEN"){
        Ok(file) => {
            let token = file.trim_end_matches('\n');
            token.to_owned()
        }
        Err(why) => {
            println!("Couldn't open ./TOKEN file, {}", why);
            loop {}
        }
    };

    loop {

        let (socket, _) = create_socket().await;
        
        println!("Connected to gateway");

        let connector = hyper_rustls::HttpsConnector::with_webpki_roots();
        let http: Client<HttpsConnector<HttpConnector>> = Builder::default().build::<HttpsConnector<HttpConnector>, Body>(connector);


        let mut client = Gateway {
            token: token.to_string(),
            user: None,
            socket,
            http,
            seq: 0,
            session_id: None,
            heartbeat_last_acked: true,
            inflater: Inflater::new(),
            authenticated: false,
            message_cache: HashMap::new(),
            sob_lb: HashMap::new(),
            snipes: HashMap::new(),
            user_cache: HashMap::new(),
        };
        client.start().await;
    }
}
  



struct Gateway {
    token: String,
    user: Option<User>,
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    http: Client<HttpsConnector<HttpConnector>>,
    seq: u64,
    session_id: Option<String>,
    heartbeat_last_acked: bool,
    inflater: inflater::Inflater,
    authenticated: bool,
    message_cache: HashMap<u64, BoundedVecDeque<Message>>,
    sob_lb: HashMap<u64, u64>,
    snipes: HashMap<u64, Message>,
    user_cache: HashMap<u64, User>,
}

impl Gateway{
    async fn start(&mut self) {
        
        let channel  = mpsc::channel::<bool>(); 

        loop {
            match tokio::time::timeout(tokio::time::Duration::from_secs(1), self.socket.next()).await {
                Ok(msg) => {
                    match msg {
                        Some(event) => {
                            match event {
                                Ok(message) => {
                                    match message {
                                        WSMessage::Binary(bytes) => {
                                            self.inflater.extend(bytes.as_slice());
                                        }
                                        WSMessage::Close(close) => {
                                            if let Some(cf) = close {
                                                println!("WS CLOSED WITH CODE {}, REASON ({})\n", cf.code, cf.reason);
                                                match cf.code.into() {
                                                    1000 | 1001 => {return}, // Unresumable Closes
                                                    4004 => {panic!("TOKEN NO LONGER VALID {:?}", cf)} // Invalid authentication
                                                    4007 | 4009 => {return}, // Something went wrong when resuming
                                                    4000..=4008 => {self.resume().await;} // Should be handled
                                                    _ => {self.resume().await;} // Unknown close, try to resume ?
                                                }
                                            } else {
                                                println!("Websocket closed with no close frame\n");
                                                return
                                            }
                                        },
                                        _ => {
                                            println!("got other message type. - {:?}", message)
                                        }
                                    }
                                }
                                Err(error) => {
                                    println!("Got error, {:?} - resuming", error);
                                    self.resume().await;
                                },
                            }
                            
                    
                            match self.inflater.msg() {
                                Ok(bytes) => {
                                    if bytes.is_none() {
                                        continue
                                    };
                
                                    match eetf::Term::decode(Cursor::new(&bytes.unwrap())) {
                                        Ok(term) => {
                                            let payload = match Payload::new(term) {
                                                Ok(payload) => payload,
                                                Err(error) => {
                                                    println!("Deserialize error, {:?}", error);
                                                    continue
                                                }
                                            };
                                            self.handle_payload(payload, &channel).await
                                        },
                                        Err(error) => println!("Decoder error {:?}", error),
                                    }
                                    
                                    self.inflater.clear();
                                }
                                Err(error) => println!("Error inflating {:?}",error),
                            }
                        },
                        None => {}, // No message - continue with loop
                    }
                },
                Err(_) => {}, // Timed out - continue with loop
            };
            match channel.1.try_recv() {
                Ok(_) => self.heartbeat().await,
                Err(error) => {
                    match error {
                        mpsc::TryRecvError::Empty => {},
                        mpsc::TryRecvError::Disconnected => println!("Heartbeat channel errored - {:?}", error),
                    }
                    
                },
            };
            
        }
    
    
    }
    
    async fn send(&mut self, op: u8, message: Data) {
        if op == 1 {
            if !self.heartbeat_last_acked {
                self.socket.close(None).await.ok();
                return
            } else {
                self.heartbeat_last_acked = false
            }
        }

        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        message.to_map().encode(&mut buf).unwrap();
        match self.socket.send(WSMessage::binary(buf)).await {
            Ok(_) => return,
            Err(error) => println!("Sending data error {}", error),
        };
    }

    fn update_cache(&mut self, payload: &Payload) {

        let data = match payload.d.d.get("user") {
            Some(user) => user,
            None => match payload.d.d.get("author") {
                Some(user) => user,
                None => return,
            },
        };

        let user = if let Value::Data(d) = data {
            if let Ok(user) = User::try_from(d) {
                user
            } else {return}
        } else {return};
        // println!("Cached updated with {}, from {:?}", user.display(), payload.t);
        self.user_cache.insert(user.id, user);
    }
    
    async fn handle_payload(&mut self, payload: Payload, channel: &(mpsc::Sender<bool>, mpsc::Receiver<bool>)) {
       
        match payload.op {
            // DISPATCH
            0 => {
                self.seq = payload.s.unwrap_or(self.seq);
                self.update_cache(&payload);
                match payload.t.clone() {
                    Some(event) => {
                        match event.as_str() {
                            "MESSAGE_CREATE" => {
                                self.message_create(payload).await
                            },
                            "READY" => {
                                self.ready(payload).await
                            },
                            "MESSAGE_REACTION_ADD" | "MESSAGE_REACTION_REMOVE" => {
                                self.update_sob_lb(payload)
                            },

                            "MESSAGE_DELETE" => {
                                self.add_snipe(payload).await
                            }
                            _ => {
                                // println!("{} - {:#?}", event, payload.d);
                            }
                        }
                    },
                    None => unreachable!(),
                };
            },

            // HEARTBEAT
            1 => self.heartbeat().await, 

            // Send only - | IDENTIFY | PRESENCE UPDATE | VOICE STATE UPDATE |
            2 | 3 | 4 | 6 => unreachable!(),

            // RECONNECT (RESUME)
            7 => self.resume().await,

            // Send only - REQUEST GUILD MEMBERS
            8 => unreachable!(),

            // INVALID SESSION
            9 => {
                self.authenticated = false;
                let resumable: bool = match payload.d.d.get("value") {
                    Some(value) => match value {
                        Value::Bool(b) => b.clone(),
                        _ => false,
                    }
                    None => false,
                };

                println!("Session invalidated, resumable: {} ({:?})", resumable, payload);
                
                if !resumable {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    self.identify(payload, None).await;
                } else {
                    self.resume().await;
                }
            },

            // HELLO
            10 => {
                self.identify(payload, Some(&channel.0)).await;
            },

            // HEARTBEAT ACK
            11 => {self.heartbeat_last_acked = true}, 

            _ => println!("got unkown payload opcode {}", payload.op),
        };
        return
    }

    async fn run_heartbeat(&mut self, payload: Payload, tx: &mpsc::Sender<bool>) {
        let interval = match payload.d.d.get("heartbeat_interval").unwrap_or(&Value::Integer(30)) {
            Value::Integer(int) => core::time::Duration::from_millis(int.clone() as u64),
            _ => unreachable!(),
        };
        
        let channel = tx.clone();
        println!("Heartbeating every {:?}", interval);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                match channel.send(true) {
                    Ok(_) => {},
                    Err(_) => {
                        println!("Heartbeater cancelled.");
                        break
                    },
                };
            }
        });
    }

    async fn heartbeat(&mut self) {
        self.send(1, 
            Data::new(
                vec![
                    ("op", Value::Integer(1)),
                    ("d", match self.seq {
                        u64::MAX => Value::NoneType,
                        _ => Value::BigInt(self.seq)
                    })
                ]
            )
        ).await
    }

    async fn resume(&mut self) {
        if self.session_id.is_none() {
            self.socket.close(None).await.ok();
            return
        }
        let (socket, _) = create_socket().await;
        self.socket = socket;
        self.inflater = Inflater::new();
        println!("Resumed at {}", self.seq);
        self.send(6,
            Data::new(
                vec![
                    ("op", Value::Integer(6)),
                    ("d", Value::Data(Data::new(
                        vec![
                            ("token", Value::String(self.token.clone())),
                            ("session_id", Value::String(self.session_id.clone().unwrap())),
                            ("seq", Value::Integer(self.seq as i32))
                        ]
                    )))
                ]
            )
        ).await
    }

    async fn add_snipe(&mut self, payload: Payload) {
        let message_id =  if let Value::BigInt(id) = payload.d.d.get("id").unwrap_or(&Value::NoneType) {
            id
        } else {
            return
        };
        let channel_id =  if let Value::BigInt(id) = payload.d.d.get("channel_id").unwrap_or(&Value::NoneType) {
            id
        } else {
            return
        };

        if let Some(queue) = self.message_cache.get(channel_id) {
            match queue.binary_search_by(|other| {other.id.cmp(message_id)}) {
                Ok(index) => {
                    let message = queue[index].clone();
                    self.snipes.insert(*channel_id, message);
                },
                Err(_) => {},
            }
        }
    }

    async fn message_create(&mut self, payload:Payload) {
        let message = match Message::try_from(&payload.d) {
            Ok(m) => m,
            Err(error) => return println!("Failed to parse message {:?}", error)
        };

        if self.message_cache.contains_key(&message.channel_id) {
            let q = self.message_cache.get_mut(&message.channel_id).unwrap();
            q.push_back(message.clone());
        } else {
            let mut cache: BoundedVecDeque<Message> = BoundedVecDeque::new(*MESSAGE_CACHE_PER_CHANNEL);
            cache.push_back(message.clone());
            self.message_cache.insert(
                message.channel_id,
                cache
            );
        }

        if let Some(current_user) = &self.user {
            if current_user.id == message.author.id {
                return
            }
        } else {
            return
        }

        // transcode

        for attachment in &message.attachments {
            if !attachment.renderable {
                if [".mov", ".mp4", ".mkv"].iter().any(|s| {attachment.filename.ends_with(s)}) {
                    self.send_message(message.channel_id, format!("{} unrenderable", attachment.filename), Some(message.id)).await;
                }
            }
        }


        if let Some(content) = message.content.clone() {
            let (command, arg) = headtail(&content);
            match command.to_lowercase().as_str() {
                "ping" => {
                    self.send_message(message.channel_id, "ping", Some(message.id)).await;
                },
                "av" => {
                    self.command_av(message, arg).await;
                },
                "soblb" => {
                    let mut leaderboard = String::with_capacity(self.sob_lb.len() * 32);

                    let mut lb = self.sob_lb.iter().collect::<Vec<(&u64,&u64)>>();
                    lb.sort_by(|a,b| {a.1.cmp(b.1)});

                    leaderboard.push_str("SOBS!!! :\n");

                    for (user, sobs) in lb.iter().rev() {
                        if sobs == &&0 {continue}
                        let d = match self.user_cache.get(user) {
                            Some(u) => u.friendly_display(),
                            None => format!("{}", user),
                        };
                        leaderboard.push_str(format!("{} - {}\n", d, sobs).as_str())
                    }

                    self.send_message(message.channel_id, leaderboard, Some(message.id)).await;
                },
                "cache" => {
                    let mut total: usize = 0;

                    for (_, v) in &self.message_cache {
                        total += v.len()
                    }

                    self.send_message(message.channel_id, format!(
                        "Message cache:\nChannels: {}\nMessages: {}\nUsers: {}",
                        self.message_cache.len(),
                        total,
                        self.user_cache.len(),
                    ), Some(message.id)).await
                }

                "snipe" => {
                    let m = self.snipes.get(&message.channel_id);
                    match m {
                        Some(message) => {
                            


                            let mut formatted = format!(
                                "{} deleted:\n\n{}\n",
                                message.author.display(),
                                message.content.as_ref().unwrap_or(&"no content".to_string())
                            );

                            for attachment in &message.attachments {
                                formatted.push_str(&attachment.url);
                                formatted.push('\n');
                            };
                            self.send_message(
                                message.channel_id,
                                formatted,
                                Some(message.id)
                            ).await;
                        }
                        None => {
                            self.send_message(
                                message.channel_id,
                                "no snipe",
                                Some(message.id)
                            ).await;
                        }
                    }
                }
                _ => {}
            }
        };



        return
    }

    fn parse_arg_user(&self, term: &str) -> Option<&User> {
        // mention regex
        let re = regex::Regex::new(r"<@!*(&*[0-9]{14,20})>").unwrap();
        if let Some(caps) = re.captures(term) {
            return self.user_cache.get(&u64::from_str_radix(&caps.get(1).unwrap().as_str(), 10).unwrap())
        }

        for user in self.user_cache.values() {
            if user.username.eq_ignore_ascii_case(term)  || user.display().eq_ignore_ascii_case(term) || user.friendly_display().eq_ignore_ascii_case(term) {
                return Some(user);
            };
        };

        None
    }

    async fn command_av(&self, message: Message, args: Option<&str>) {
        match args {
            Some(arg) => {
                if let Some(user) = self.parse_arg_user(arg) {
                    self.send_message(message.channel_id, user.avatar(4096), Some(message.id)).await;
                } else {
                    self.send_message(message.channel_id, "Couldnt find that user", Some(message.id)).await;
                }
            },
            None => {
                self.send_message(message.channel_id, message.author.avatar(4096), Some(message.id)).await;
            }
        }

    }

    fn update_sob_lb(&mut self, payload: Payload) {
        let react = match Reaction::try_from(&payload) {
            Ok(r) => r,
            Err(error) => return println!("Failed to parse react {:?}", error)
        };

        match react {
            Reaction::Add(event) => {
                if event.emoji.name != "😭" {return}

                if let Some(queue) = self.message_cache.get(&event.channel) {
                    match queue.binary_search_by(|other| {other.id.cmp(&event.message)}) {
                        Ok(index) => {
                            let user_id = queue[index].author.id;
                            if event.reactor == user_id {return}
                            *self.sob_lb.entry(user_id).or_insert(0) += 1;
                        },
                        Err(_) => {},
                    }
                }
            },
            Reaction::Remove(event) => {
                if event.emoji.name != "😭" {return}

                if let Some(queue) = self.message_cache.get(&event.channel) {
                    match queue.binary_search_by(|other| {other.id.cmp(&event.message)}) {
                        Ok(index) => {
                            let user_id = queue[index].author.id;
                            if event.reactor == user_id {return}
                            *self.sob_lb.entry(user_id).or_insert(1) -= 1;
                        },
                        Err(_) => {},
                    }
                }
            },
        }

    }

    async fn send_message<T: Into<String>>(&self, channel: u64, message_content: T, reference: Option<u64>) {
        let content: String = message_content.into();
        let route = format!("https://discord.com/api/v9/channels/{}/messages", channel);
        let mut body = object! {
            content: content,
            tts: false,
        };
        if let Some(referenced_msg) = reference {
            body.insert("message_reference", object! {
                message_id: referenced_msg.to_string(),
                fail_if_not_exists: false,
            }).unwrap();
        };

        match self.send_as_json(route, body, "POST").await {
            Ok(_) => {}
            Err(e) => println!("Http error sending to <#{}> - {:?}", channel, e)
        };
    }

    async fn send_as_json(&self, uri: String, body: json::JsonValue, method: &str) -> Result<http::Response<Body>, hyper::Error> {
        let req = self.request(uri)
            .method(method)
            .body(Body::from(body.dump()))
            .unwrap();

        self.http.request(req).await
    }

    async fn ready(&mut self, payload: Payload) {
        if let Some(value) = payload.d.d.get("session_id") {
            if let Value::String(string) = value {
                self.session_id = Some(string.clone())
            }
        }
        
        if let Some(value) = payload.d.d.get("user") {
            if let Value::Data(d) = value {


                match User::try_from(d) {
                    Ok(u) => {
                        println!("READY {}", u.display());
                        self.user = Some(u)
                    },
                    Err(error) => println!("Failed to parse current user {:?} {:?}", d, error)
                };
                return
            }
            
        };
        println!("Ready as ?? {:?}", payload);
    }

    async fn identify(&mut self, payload: Payload, channel: Option<&mpsc::Sender<bool>>) {
        if self.authenticated {return};
        let data = self.identify_payload();
        self.send(2, data).await;
        if let Some(tx) = channel {
            self.run_heartbeat(payload, tx).await;
        }
        self.authenticated = true;
        
        self.set_presence().await;
    }


    async fn set_presence(&mut self) {
        let payload = Data::new(
            vec![
                ("op", Value::Integer(3)),
                ("d", Value::Data(Data::new(
                    vec![
                        ("status", Value::String("idle".to_string())),
                        ("since", Value::Integer(0)),
                        ("activities", Value::Array(vec![
                            Value::Data(Data::new(vec![
                                ("emoji", Value::NoneType),
                                ("name", Value::String("Custom Status".to_string())),
                                ("state", Value::String(":3".to_string())),
                                ("type", Value::Integer(4))
                            ]))
                        ])),
                        ("afk", Value::Bool(false)),
                    ]
                ))),
            ]
        );

        self.send(3, payload).await;
    }

    fn request(&self, uri: String) -> hyper::http::request::Builder {
        Request::builder()
            .uri(uri)
            .header("Accept", "*/*")
            .header("Accept-Encoding", "gzip, deflate, br")
            .header("Accept-Language", "en-GB")
            .header("Authorization", self.token.clone())
            .header("Content-Type", "application/json")
            .header("Origin", "https://discord.com")
            .header("Referer", "https://discord.com/app")
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-origin")
            .header("User-Agent", format!("Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) discord/{} Chrome/83.0.4103.122 Electron/9.3.5 Safari/537.36", CLIENT_VERSION))
            .header("X-Super-Properties", "nosniff")
    }

    fn identify_payload(&self) -> Data {

        Data::new(vec![
            ("op", Value::Integer(2)),
            ("d", Value::Data(Data::new(
                vec![
                    ("token", Value::String(self.token.to_string())),
                    ("capabilities", Value::Integer(125)),
                    ("properties", Value::Data(Data::new(
                        vec![
                            ("os", Value::String("Windows".to_string())),
                            ("Browser", Value::String("Discord Client".to_string())),
                            ("release_channel", Value::String("stable".to_string())),
                            ("client_version", Value::String(CLIENT_VERSION.to_string())),
                            ("os_version", Value::String("10.0.19041".to_string())),
                            ("system_locale", Value::String("en-GB".to_string())),
                            ("client_build_number", Value::Integer(*CLIENT_BUILD_NUMBER)),
                            ("client_event_source", Value::NoneType)
                        ]
                    ))),
                    ("presence", Value::Data(Data::new(
                        vec![
                            ("status", Value::String("online".to_string())),
                            ("since", Value::Integer(0)),
                            ("activities", Value::Array(vec![])),
                            ("afk", Value::Bool(false)),
                        ]
                    ))),
                    ("compress", Value::Bool(false)),
                    ("client_state", Value::Data(Data::new(
                        vec![
                            ("guild_hashes", Value::Data(Data::new::<String>(vec![]))),
                            ("highest_last_message_id", Value::String("0".to_string())),
                            ("read_state_version", Value::Integer(0)),
                            ("user_guild_settings_version", Value::Integer(-1)),
                        ]
                    )))
                ]
            )))
        ])
    }
}