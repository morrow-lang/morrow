//! Fast global authentication; no Fern execution or checkpoint IO runs here.
use super::*;

#[derive(Clone)]
pub(crate) struct Authentication {
    pub token: String,
    pub csrf: String,
    pub revoked: watch::Receiver<bool>,
    expires: Instant,
}
impl Authentication {
    pub fn capability(&self) -> Capability {
        Capability {
            principal: self.token.clone(),
            revoked: self.revoked.clone(),
            expires: self.expires,
        }
    }
}
#[derive(Clone)]
pub(crate) struct Capability {
    pub principal: String,
    revoked: watch::Receiver<bool>,
    expires: Instant,
}
impl Capability {
    pub fn valid(&self) -> bool {
        Instant::now() < self.expires
            && !*self.revoked.borrow()
            && self.revoked.has_changed().is_ok()
    }
}
struct Session {
    csrf: String,
    expires: Instant,
    revoke: watch::Sender<bool>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.revoke.send_replace(true);
    }
}

pub(super) struct AuthenticationOwner {
    config: Config,
    sessions: BTreeMap<String, Session>,
}
impl AuthenticationOwner {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            sessions: BTreeMap::new(),
        }
    }
    pub fn expire(&mut self) {
        self.sessions
            .retain(|_, session| session.expires > Instant::now());
    }
    pub fn authentication(
        &self,
        token: &str,
        csrf: Option<&str>,
    ) -> Result<Authentication, StatusCode> {
        let session = self
            .sessions
            .get(token)
            .filter(|session| session.expires > Instant::now())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        if csrf.is_some_and(|csrf| !same_secret(csrf, &session.csrf)) {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(Authentication {
            token: token.into(),
            csrf: session.csrf.clone(),
            revoked: session.revoke.subscribe(),
            expires: session.expires,
        })
    }
    pub fn login(&mut self, key: &str) -> Result<Authentication, StatusCode> {
        self.expire();
        if !same_secret(key, &self.config.access_key) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        if self.sessions.len() >= self.config.max_sessions {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let token = token().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let csrf = super::token().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let (revoke, revoked) = watch::channel(false);
        let expires = Instant::now() + self.config.session_ttl;
        self.sessions.insert(
            token.clone(),
            Session {
                csrf: csrf.clone(),
                expires,
                revoke,
            },
        );
        Ok(Authentication {
            token,
            csrf,
            revoked,
            expires,
        })
    }
    pub fn handle(&mut self, request: Request) {
        self.expire();
        match request {
            Request::Login { key, reply } => {
                if let Err(Ok(authentication)) = reply.send(self.login(&key)) {
                    // A timed-out requester cannot use credentials it never
                    // received; do not retain its admission slot until expiry.
                    self.sessions.remove(&authentication.token);
                }
            }
            Request::Authenticate { token, csrf, reply } => {
                let _ = reply.send(self.authentication(&token, csrf.as_deref()));
            }
            Request::Logout { token, csrf, reply } => {
                let result = self.authentication(&token, Some(&csrf)).map(|_| {
                    self.sessions.remove(&token);
                });
                let _ = reply.send(result);
            }
            _ => unreachable!("dispatcher sends only authentication requests"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_login_does_not_retain_an_unpublished_session() {
        let mut config = Config::new("http://localhost".into(), "long-enough-test-key".into());
        config.max_sessions = 1;
        let mut owner = AuthenticationOwner::new(config);
        let (reply, response) = oneshot::channel();
        drop(response);
        owner.handle(Request::Login {
            key: "long-enough-test-key".into(),
            reply,
        });
        assert_eq!(
            owner.sessions.len(),
            0,
            "the client cannot use an unpublished credential"
        );
        assert!(owner.login("long-enough-test-key").is_ok());
    }
}
