use bit_set::BitSet;
use crossbeam_channel as chan;
use std::sync::{Arc, Mutex};
use tracing::{debug, error};

pub type TokenValue = usize;

#[derive(Debug)]
pub enum TokenUpdate<T> {
    Acquire(TokenValue, T),
    Release(TokenValue),
}

struct TokensInner<T> {
    data: Mutex<BitSet<u64>>,
    evc: chan::Sender<TokenUpdate<T>>,
}

#[derive(Clone)]
pub struct Tokens<T>(Arc<TokensInner<T>>);

pub struct TokenGuard<T> {
    parent: Arc<TokensInner<T>>,
    tokval: TokenValue,
}

impl<T> Drop for TokenGuard<T> {
    fn drop(&mut self) {
        let tv = self.tokval;
        let mut is_success = false;
        if let Ok(mut tks) = self.parent.data.lock() {
            if tks.insert(tv) {
                is_success = true;
            } else {
                error!("unable to return token {}", tv);
            }
        } else {
            error!("tokens are poisoned");
        }
        if is_success {
            let _ = self.parent.evc.send(TokenUpdate::Release(tv));
            debug!("released token {}", tv);
        }
    }
}

impl<T> Tokens<T> {
    pub fn new(evc: chan::Sender<TokenUpdate<T>>) -> Self {
        let ibs: BitSet<_> = (0..(u16::MAX - 1) as TokenValue).into_iter().collect();
        Tokens(Arc::new(TokensInner {
            data: Mutex::new(ibs),
            evc,
        }))
    }

    pub fn try_acquire(&self, data: T) -> Result<TokenGuard<T>, T> {
        let tokval = match self.0.data.lock() {
            Ok(mut tks) => {
                if let Some(tokval) = tks.iter().next() {
                    tks.remove(tokval);
                    tokval
                } else {
                    return Err(data);
                }
            }
            Err(_) => return Err(data),
        };
        let _ = self.0.evc.send(TokenUpdate::Acquire(tokval, data));
        Ok(TokenGuard {
            parent: Arc::clone(&self.0),
            tokval,
        })
    }
}
