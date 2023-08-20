use std::{collections::HashMap, fmt,};

use eetf::{Atom, BigInteger, Binary, FixInteger, List, Map, Term, Float};

#[derive(Debug)]
pub enum SerialiseError {
    BadSequence,
    BadKey,
    IncompletePayload,
    NonMapTerm,
}

#[derive(Debug, Clone)]
pub struct Payload {
    pub d: Data,
    pub op: u8,
    pub s: Option<u64>,
    pub t: Option<String>
}

impl Payload {
    pub fn new(term:Term) -> Result<Self, SerialiseError> {
        match term {
            Term::Map(map) => {
                let mut d: Option<Term> = None;
                let mut op: Option<u8> = None;
                let mut s: Option<Option<u64>> = None;
                let mut t: Option<Option<String>> = None;

                for entry in map.entries.iter() {
                    match &entry.0 {
                        Term::Atom(atom) => {
                            match atom.name.as_ref() {
                                "d" => {d = Some(entry.1.clone())},
                                "op" => {
                                    if let Term::FixInteger(int) = &entry.1 {
                                        op = Some(int.value as u8)
                                    }
                                },
                                "s" => {
                                    s = match &entry.1 {
                                        Term::Atom(atom) =>{
                                            if atom.name == "nil" {
                                                Some(None)
                                            } else {
                                                None
                                            }
                                        }
                                        Term::FixInteger(int) => {
                                            Some(Some(int.value.clone() as u64))
                                        }
                                        _ => return Err(SerialiseError::BadSequence)
                                    }
                                }
                                "t" => {
                                    if let Term::Atom(atom) = &entry.1 {
                                        if atom.name == "nil"{
                                            t = Some(None)
                                        } else {
                                            t = Some(Some(atom.name.clone()))
                                        }
                                    }
                                },
                                _ => println!("unknown field")
                            }
                        }
                        _ => return Err(SerialiseError::BadKey)
                    }
                }

                if d.is_some() && op.is_some() && s.is_some() && t.is_some() {
                    let d = d.unwrap();
                    let op = op.unwrap();
                    let s = s.unwrap();
                    let t = t.unwrap();

                    return Ok(Payload {
                        d: Data::from_map(d),
                        op,
                        s,
                        t,
                    })
                } else {
                    return Err(SerialiseError::IncompletePayload)
                }
            },
            _ => return Err(SerialiseError::NonMapTerm)
        }
    }
}

#[derive(Clone)]
pub enum Value {
    String(String),
    Integer(i32),
    Bool(bool),
    BigInt(u64),
    Data(Data),
    Array(Vec<Self>),
    Float(f64),
    NoneType,
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::String(string) => write!(f, "'{}'", string),
            Value::Integer(int) => write!(f, "I{}", int),
            Value::Bool(bool) => write!(f, "b{}", bool),
            Value::BigInt(int) => write!(f, "B{}", int),
            Value::Data(data) => write!(f, "D{:?}", data),
            Value::Array(vec) => write!(f, "A{:?}", vec),
            Value::Float(flt) => write!(f, "f{:?}", flt),
            Value::NoneType => write!(f, "None"),
        }
    }
}

impl std::default::Default for Value {
    fn default() -> Self {
        Value::NoneType
    }
}

impl std::default::Default for &Value {
    fn default() -> Self {
        &Value::NoneType
    }
}

#[derive(Clone)]
pub struct Data {
    pub d: HashMap<String, Value>,
}

impl fmt::Debug for Data {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.d)
    }
}

impl Data {
    pub fn new<T>(hash: Vec<(T, Value)>) -> Self 
    where T: ToString + Clone
    {
        Self {
            d: hash.iter().map(|e| {(e.0.to_string(), e.1.clone())}).collect::<HashMap<String, Value>>(),
        }
    }

    pub fn from_map(term: Term) -> Self {
        match term {
            Term::Map(map) => {
                let mut d: HashMap<String, Value> = HashMap::new();
                for (k, v) in map.entries.iter() {
                    d.insert(
                        match k {
                            Term::Atom(atom) => {
                                atom.name.clone()
                            },
                            Term::Binary(bytes) => {
                                String::from_utf8(bytes.bytes.clone()).unwrap()
                            }
                            _ => panic!(),
                        },
                        Self::from_term(v),
                    );
                };
                Data {
                    d,
                }
            },
        
            Term::List(list) => {
                if list.elements.len() > 1 {
                    println!("throwing out list elements in deserialisation",);
                };

                Self::from_map(list.elements[0].clone())
            },

            Term::Atom(atom) => {
                match atom.name.as_str() {
                    "nil" => Data {d: HashMap::new()},
                    "true" => { Data::new(
                        vec![
                            ("value", Value::Bool(true)),
                        ]
                    )},
                    "false" => {
                        Data::new(
                            vec![
                                ("value", Value::Bool(false))
                            ]
                        )
                    },
                    _ => panic!("Unexpected atom {}", atom.name),
                    }
                },
            _ => panic!("Unexpected term time {:?}", term),
        }
    }

    fn from_term(term: &Term) -> Value {
        match term {
            Term::Atom(atom) => {
                match atom.name.parse::<bool>() {
                    Ok(result) => Value::Bool(result),
                    Err(_) => {
                        if atom.name == "nil" {
                            Value::NoneType
                        } else {
                            Value::String(atom.name.clone())
                        }
                    },
                }
            },
            Term::BigInteger(bigint) => Value::BigInt(bigint.value.to_string().parse().unwrap()),
            Term::FixInteger(int) => Value::Integer(int.value),
            Term::List(list) => Value::Array(list.elements.clone().iter().map(|t| {Self::from_term(t)}).collect::<Vec<Value>>()),
            Term::Map(_) => Value::Data(Data::from_map(term.clone())),
            Term::Binary(bytes) => {
                let s = String::from_utf8(bytes.bytes.clone()).unwrap();
                if s == "" || s == "nil" {
                    Value::NoneType
                } else {
                    Value::String(s)
                }
            },
            Term::Float(f) => Value::Float(f.value),
            _ => panic!("unexpected type : {:?}", term),
            
            
        }
    }

    pub fn to_map(&self) -> Term {
        let mut out: Vec<(Term, Term)> = Vec::with_capacity(8);
        for (k, v) in self.d.iter() {
            out.push(
                (
                    Term::from(Binary::from(k.clone().as_bytes())),
                    Self::to_term(v)
                )
            )
        }
        Term::from(Map::from(out))
    }

    fn to_term(value: &Value) -> Term {
        match value {
            Value::String(string) => Term::from(Binary::from(string.clone().as_bytes())),
            Value::Integer(int) => Term::from(FixInteger::from(int.clone())),
            Value::Bool(boolean) => Term::from(Atom::from(format!("{}",boolean))),
            Value::BigInt(int) => Term::from(BigInteger::from(int.clone())),
            Value::Data(d) => Term::from(d.to_map().clone()),
            Value::Array(array) => Term::from(List::from(array.clone().iter().map(|v| {Self::to_term(v)}).collect::<Vec<Term>>())),
            Value::Float(flt) => Term::from(Float {value: *flt}), // if its not finite idc
            Value::NoneType => Term::from(Atom::from("nil")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: u64,
    pub channel_id: u64,
    pub author: User,
    pub content: Option<String>,
    pub attachments: Vec<Attachment>,
    pub message_type: u64,
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: u64,
    pub displayname: Option<String>,
    pub username: String,
    pub discriminator: u16,
    avatar_hash: Option<String>,
    banner_hash: Option<String>,
    pub bot: bool,
    pub flags: u64,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: u64,
    pub filename: String,
    pub url: String,
    pub renderable: bool,
    pub size: u64,
    pub dimensions: Option<(u32, u32)>,
    pub content_type: Option<String>,
}

#[derive(Debug)]
pub enum ParseError {
    BadSubObject,
    MissingId,
    MissingParam,
    BadParam,
}

impl TryFrom<&Data> for Message {
    type Error = ParseError;

    fn try_from(payload: &Data) -> Result<Self, Self::Error> {

        let user = match {
            if let Value::Data(user_data) = payload.d.get("author").unwrap_or_default() {
                User::try_from(user_data)
            } else {
                Err(ParseError::BadSubObject)
            }
        } {
            Ok(value) => value,
            Err(e) => return Err(e),
        };
        
        let attachments = match payload.d.get("attachments").unwrap_or_default()  {
            Value::Array(attachment_array) => {
                attachment_array.iter().map(|v| {
                    if let Value::Data(attachment) = v {
                        Attachment::try_from(attachment).ok()
                    } else {
                        None
                    }
                }).filter_map(std::convert::identity).collect::<Vec<Attachment>>()
            },
            _ => return Err(ParseError::BadSubObject)
        };

        let id = match payload.d.get("id").unwrap_or_default() {
                Value::BigInt(value) => value.clone(),
                _ => return Err(ParseError::MissingParam)
        };

        let channel_id = match payload.d.get("channel_id").unwrap_or_default() {
            Value::BigInt(value) => value.clone(),
            _ => return Err(ParseError::MissingParam)
        };

        let message_type = match payload.d.get("type").unwrap_or_default() {
            Value::Integer(value) => *value as u64,
            _ => return Err(ParseError::MissingParam)
        };

        let content = match payload.d.get("content").unwrap_or_default() {
            Value::String(content) => Some(content.clone()),
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam)
        };

        return Ok(Message{
            id,
            channel_id,
            author: user,
            content,
            attachments,
            message_type,
        });

    } 
}

impl TryFrom<&Data> for User {
    type Error = ParseError;

    fn try_from(payload: &Data) -> Result<Self, Self::Error> {

        let id = match payload.d.get("id").unwrap_or_default() {
            Value::BigInt(value) => value.clone(),
            _ => return Err(ParseError::MissingId),
        };

        let username = match payload.d.get("username").unwrap_or_default() {
            Value::String(value) => value.clone(),
            _ => return Err(ParseError::MissingParam),
        };

        let displayname = match payload.d.get("global_name").unwrap_or_default() {
            Value::String(value) => Some(value.clone()),
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam),
        };

        let discrim = match payload.d.get("discriminator").unwrap_or_default() {
            Value::String(value) => {
                match value.parse::<u16>() {
                    Ok(discrim) => discrim,
                    Err(_) => return Err(ParseError::BadParam),
                }
            },
            _ => return Err(ParseError::MissingParam),
        };

        let avatar = match payload.d.get("avatar").unwrap_or_default() {
            Value::String(value) => Some(value.clone()),
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam),
        };

        let banner = match payload.d.get("banner").unwrap_or_default() {
            Value::String(value) => Some(value.clone()),
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam),
        };

        let flags = match payload.d.get("flags").unwrap_or_default() {
            Value::Integer(value) => value.clone() as u64,
            Value::NoneType => {
                match payload.d.get("public_flags").unwrap_or_default() {
                    Value::Integer(value) => value.clone() as u64,
                    _ => return Err(ParseError::MissingParam),
                }
            },
            _ => return Err(ParseError::BadParam),
        };

        let bot = match 
        payload.d.get("bot").unwrap_or_default() {
            Value::Bool(b) => b.clone(),
            Value::NoneType => false,
            _ => return Err(ParseError::BadParam)
        };

        Ok(User{
            id,
            username,
            displayname,
            discriminator: discrim,
            avatar_hash: avatar,
            banner_hash: banner,
            bot,
            flags,
        })


    }

}

impl User {
    pub fn avatar(&self, size: u16) -> String {
        if let Some(av) = self.avatar_hash.as_ref() {
           let ext = match av.starts_with("a_") {
            true => "gif",
            false => "jpg",
        };
        format!("https://cdn.discordapp.com/avatars/{}/{}.{}?size={}",self.id, av, ext, size) 
        } else {
            if self.discriminator != 0 {
                format!("https://cdn.discordapp.com/embed/avatars/{}.png", self.discriminator % 5)
            } else {
                format!("https://cdn.discordapp.com/embed/avatars/{}.png", (self.id >> 22) % 6)
            }
        }
        
    }

    pub fn banner(&self, size: u16) -> Option<String> {
        if let Some(banner) = self.banner_hash.as_ref() {
            let ext = match banner.starts_with("a_") {
                true => "gif",
                false => "jpg"
            };
            Some(format!("https://cdn.discordapp.com/banners/{}/{}.{}?size={}",self.id, banner, ext, size))
        } else {
            None
        }
    }
    
    pub fn friendly_display(&self) -> String {
        if let Some(dname) = &self.displayname {
            dname.clone()
        } else {
            self.display()
        }
        
    }

    
    pub fn display(&self) -> String {
        if self.discriminator != 0 {
            format!("{}#{}", self.username, self.discriminator)
        } else {
            self.username.clone()
        }
        
    }
}

impl TryFrom<&Data> for Attachment {
    type Error = ParseError;

    fn try_from(payload: &Data) -> Result<Self, Self::Error> {
        let id = match payload.d.get("id").unwrap_or_default() {
            Value::BigInt(value) => value.clone(),
            _ => return Err(ParseError::MissingId),
        };

        let filename = match payload.d.get("filename").unwrap_or_default() {
            Value::String(value) => value.clone(),
            _ => return Err(ParseError::MissingParam),
        };

        let size = match payload.d.get("size").unwrap_or_default() {
            Value::Integer(value) => *value as u64,
            _ => return Err(ParseError::MissingId),
        };

        let url = match payload.d.get("url").unwrap_or_default() {
            Value::String(value) => value.clone(),
            _ => return Err(ParseError::MissingParam),
        };

        let height = match payload.d.get("height").unwrap_or_default() {
            Value::Integer(value) => Some(*value as u32),
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam)
        };

        let width = match payload.d.get("width").unwrap_or_default() {
            Value::Integer(value) => Some(*value as u32),
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam)
        };

        let renderable = width.is_some() & height.is_some();

        let content_type = match payload.d.get("content_type").unwrap_or_default() {
            Value::String(value) => Some(value.clone()),
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam),
        };

        let dimensions = match renderable {
            true => Some((width.unwrap(), height.unwrap())),
            false => None
        };

        Ok(Attachment{
            id,
            filename,
            url,
            renderable,
            size,
            dimensions,
            content_type,
        })
    }

}

#[derive(Debug, Clone)]
pub struct ReactionEvent {
    pub channel: u64,
    pub reactor: u64,
    pub message: u64,
    pub emoji: Emoji,
}

pub enum Reaction {
    Add(ReactionEvent),
    Remove(ReactionEvent)
}

impl TryFrom<&Payload> for Reaction {
    type Error = ParseError;

    fn try_from(payload: &Payload) -> Result<Self, Self::Error> {

        let data = &payload.d;

        let user = match data.d.get("user_id").unwrap_or_default() {
            Value::BigInt(value) => value.clone(),
            _ => return Err(ParseError::MissingId),
        };
        
        let channel = match data.d.get("channel_id").unwrap_or_default() {
            Value::BigInt(value) => value.clone(),
            _ => return Err(ParseError::MissingId),
        };

        let message = match data.d.get("message_id").unwrap_or_default() {
            Value::BigInt(value) => value.clone(),
            _ => return Err(ParseError::MissingId),
        };

        let emoji = if let Value::Data(value) = data.d.get("emoji").unwrap_or_default() {
            match Emoji::try_from(value) {
                Ok(e) => e,
                Err(_) => return Err(ParseError::BadSubObject),
            }
        } else {
            return Err(ParseError::BadSubObject)
        };

        if let Some(event) = &payload.t {
            match event.as_str() {
                "MESSAGE_REACTION_ADD" => {
                    Ok(Reaction::Add(
                        ReactionEvent {
                            channel,
                            message,
                            reactor: user,
                            emoji,
                        }
                    ))
                },
                "MESSAGE_REACTION_REMOVE" => {
                    Ok(Reaction::Remove(
                        ReactionEvent {
                            channel,
                            message,
                            reactor: user,
                            emoji,
                        }
                    ))
                },
                _ => return Err(ParseError::BadParam)
            }
        } else {
            return Err(ParseError::BadParam)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Emoji {
    pub name: String,
    pub id: Option<u64>,
}

impl TryFrom<&Data> for Emoji {
    type Error = ParseError;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        let name = match data.d.get("name").unwrap_or_default() {
            Value::String(value) => value.clone(),
            _ => return Err(ParseError::MissingParam),
        };

        let id = match data.d.get("id").unwrap_or_default() {
            Value::BigInt(value) => Some(value.clone()),
            Value::String(_) => None, // STRING 'nil' WHY
            Value::NoneType => None,
            _ => return Err(ParseError::MissingParam)
        };


        Ok(
            Emoji {
                name,
                id,
            }
        )
    }
}