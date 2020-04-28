use crossbeam_channel as chan;
use std::thread::spawn;
use std::time::{Duration, Instant};
use tracing::debug;

mod gather;
mod parser;
mod tokens;

static OSPF_PROTOS: &[&str] = &["ytrizja", "ytrizja_v6"];

use crate::tokens::{TokenGuard, TokenValue, Tokens};

enum Handler {
    PreOpen {
        ws_sender: ws::Sender,
        tokens: Tokens<ws::Sender>,
    },
    PreRunning,
    Running {
        tg: TokenGuard<ws::Sender>,
    },
    Closed,
}

impl ws::Handler for Handler {
    fn on_open(&mut self, shake: ws::Handshake) -> ws::Result<()> {
        match std::mem::replace(self, Handler::PreRunning) {
            Handler::PreOpen { ws_sender, tokens } => {
                if let Some(addr) = shake.remote_addr()? {
                    debug!("Connection with {} now open", addr);
                }
                *self = Handler::Running {
                    tg: tokens
                        .try_acquire(ws_sender)
                        .expect("unable to acquire token"),
                };
                Ok(())
            }
            _ => panic!("tried to open already opened Handler object"),
        }
    }

    fn on_close(&mut self, code: ws::CloseCode, reason: &str) {
        debug!("Connection closing due to ({:?}) {}", code, reason);
        if let Handler::Running { tg } = std::mem::replace(self, Handler::Closed) {
            std::mem::drop(tg);
        } else {
            panic!("tried to close not running Handler object");
        }
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let (s_tkinf, r_tkinf) = chan::unbounded();
    let tokens = Tokens::new(s_tkinf);

    spawn(move || {
        use rand::prelude::*;
        let mut prev_hash = None;
        let mut senders: std::collections::BTreeMap<TokenValue, ws::Sender> = Default::default();
        let mut rng = rand::thread_rng();
        loop {
            let sel_start = Instant::now();
            let mut timeout = chan::after(Duration::from_secs(10));

            // update data regulary
            let mut got_update = false;
            if let Some(dath) = gather::gather(OSPF_PROTOS) {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                dath.hash(&mut hasher);
                let new_hash = hasher.finish();
                // only report update if hash mismatches
                if std::mem::replace(&mut prev_hash, Some(new_hash)) != Some(new_hash) {
                    if senders.is_empty() {
                        // wait for new Handler to appear
                        timeout = chan::never();
                    } else {
                        let mut ids: Vec<TokenValue> = senders.keys().copied().collect();
                        ids.shuffle(&mut rng);
                        // every websocket client gets the update
                        senders
                            .get_mut(&ids.pop().unwrap())
                            .unwrap()
                            .broadcast(dath)
                            .expect("ws_sender.broadcast failed");
                        got_update = true;
                    }
                }
            }
            if !senders.is_empty() && !got_update {
                // ping everybody
                for i in senders.values_mut() {
                    i.ping(Vec::new()).expect("ws_sender.ping failed");
                }
            }

            // don't loop too fast
            while sel_start.elapsed() < Duration::from_millis(100) {
                use crate::tokens::TokenUpdate;
                chan::select! {
                    recv(r_tkinf) -> tkinf => {
                        match tkinf {
                            Err(_) => break,
                            Ok(TokenUpdate::Acquire(t, s)) => {
                                prev_hash = None;
                                senders.insert(t, s);
                            },
                            Ok(TokenUpdate::Release(t)) => {
                                senders.remove(&t);
                            },
                        }
                    },
                    recv(timeout) -> _ => {},
                }
            }
        }
    });

    ws::listen("127.0.0.1:8942", |ws_sender| Handler::PreOpen {
        tokens: tokens.clone(),
        ws_sender,
    })
    .expect("unable to launch WebSocket listener");
}
