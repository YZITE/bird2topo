use crossbeam_channel as chan;
use tracing::debug;

pub enum Handler {
    PreOpen {
        thinf_chan: chan::Sender<()>,
        upd_chan: chan::Receiver<String>,
        ws_sender: ws::Sender,
    },
    Running {
        is_closed: chan::Sender<()>,
    },
    Closed,
}

impl ws::Handler for Handler {
    fn on_open(&mut self, shake: ws::Handshake) -> ws::Result<()> {
        let (s_is_closed, r_is_closed) = chan::bounded(0);
        let (thinf_chan, upd_chan, ws_sender) = match std::mem::replace(
            self,
            Handler::Running {
                is_closed: s_is_closed,
            },
        ) {
            Handler::PreOpen {
                thinf_chan,
                upd_chan,
                ws_sender,
            } => (thinf_chan, upd_chan, ws_sender),
            _ => panic!("tried to open already opened Handler object"),
        };
        if let Some(addr) = shake.remote_addr()? {
            debug!("Connection with {} now open", addr);
        }
        std::thread::spawn(move || {
            // notify master that we want data
            thinf_chan.send(()).unwrap();
            loop {
                chan::select! {
                    recv(r_is_closed) -> _ => break,
                    // everyone should get the update
                    recv(upd_chan) -> dath => ws_sender
                        .broadcast(dath.expect("upd_chan closed"))
                        .expect("ws_sender.broadcast failed"),
                    default(std::time::Duration::from_secs(30)) =>
                        ws_sender.ping(Vec::new())
                        .expect("ws_sender.ping failed"),
                }
            }
        });
        Ok(())
    }

    fn on_close(&mut self, code: ws::CloseCode, reason: &str) {
        debug!("Connection closing due to ({:?}) {}", code, reason);
        if let Handler::Running { is_closed } = std::mem::replace(self, Handler::Closed) {
            std::mem::drop(is_closed);
        } else {
            panic!("tried to close not running Handler object");
        }
    }
}
