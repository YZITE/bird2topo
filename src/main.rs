use crossbeam_channel as chan;
use std::thread::spawn;
use std::time::{Duration, Instant};

mod gather;
mod handler;
mod parser;

static OSPF_PROTOS: &[&str] = &["ytrizja", "ytrizja_v6"];

fn main() {
    tracing_subscriber::fmt::init();

    let (s_upd, r_upd) = chan::bounded(0);
    let (s_thinf, r_thinf) = chan::bounded(1);

    spawn(move || {
        let mut prev_hash = None;
        loop {
            // update data regulary
            if let Some(dath) = gather::gather(OSPF_PROTOS) {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                dath.hash(&mut hasher);
                let new_hash = hasher.finish();
                // only report update if hash mismatches
                if std::mem::replace(&mut prev_hash, Some(new_hash)) != Some(new_hash) {
                    if s_upd.send_timeout(dath, Duration::from_secs(10)).is_err() {
                        // got timeout
                        // wait for new Handler to appear
                        let _ = r_thinf.recv();
                        continue;
                    }
                }
            }

            // don't loop too fast
            let sel_start = Instant::now();
            let timeout = chan::after(Duration::from_secs(10));
            while sel_start.elapsed() < Duration::from_millis(100) {
                chan::select! {
                    recv(r_thinf) -> thinf => {
                        match thinf {
                            Err(_) => break,
                            Ok(()) => prev_hash = None,
                        }
                    },
                    recv(timeout) -> _ => {},
                }
            }
        }
    });

    ws::listen("127.0.0.1:8942", |ws_sender| handler::Handler::PreOpen {
        thinf_chan: s_thinf.clone(),
        upd_chan: r_upd.clone(),
        ws_sender,
    })
    .expect("unable to launch WebSocket listener");
}
