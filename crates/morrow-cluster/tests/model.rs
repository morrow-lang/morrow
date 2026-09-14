//! Independent virtual-clock expectations; test source predates implementation.
//! Execution and genuine red-before-green evidence are pending disk recovery.
use morrow_cluster::*;
fn limits() -> Limits {
    Limits {
        pending_requests: 3,
        queued_bytes: 100,
        request_timeout_ms: 10,
        heartbeat_timeout_ms: 30,
        max_lease_ms: 50,
    }
}
fn link() -> LinkState {
    LinkState::new(LinkId::new([1; 16]).unwrap(), limits(), 50, 0).unwrap()
}
#[test]
fn admission_is_atomic_and_ack_releases_exact_accounting() {
    let mut link = link();
    let first = link.admit(40, 0).unwrap();
    let second = link.admit(60, 1).unwrap();
    assert_eq!(link.admit(1, 2), Err(Error::Overloaded));
    assert_eq!(
        link.usage(),
        Usage {
            pending_requests: 2,
            queued_bytes: 100
        }
    );
    link.ack(first, 2).unwrap();
    assert_eq!(link.usage().queued_bytes, 60);
    assert_eq!(link.ack(first, 3), Err(Error::UnknownRequest));
    link.ack(second, 3).unwrap();
    assert_eq!(link.usage(), Usage::default());
}
#[test]
fn loss_is_uncertain_never_a_retry_and_old_replies_cannot_attach() {
    let mut old = link();
    let request = old.admit(10, 0).unwrap();
    let loss = old.disconnect(1).unwrap();
    assert_eq!(loss.requests, vec![request]);
    assert!(loss.link_lost);
    assert_eq!(old.usage(), Usage::default());
    assert_eq!(old.admit(1, 2), Err(Error::Disconnected));
    let mut new = LinkState::new(LinkId::new([2; 16]).unwrap(), limits(), 50, 2).unwrap();
    let replacement = new.admit(10, 2).unwrap();
    assert_eq!(replacement.sequence(), 1);
    assert_eq!(new.ack(request, 3), Err(Error::StaleLink));
    assert_eq!(new.usage().pending_requests, 1);
    assert!(!old.disconnect(3).unwrap().link_lost);
}
#[test]
fn absolute_deadlines_clock_regression_and_late_outcomes() {
    let mut link = link();
    let request = link.admit(10, 0).unwrap();
    assert!(link.expire(9).unwrap().requests.is_empty());
    assert_eq!(link.ack(request, 10), Err(Error::RequestExpired));
    assert_eq!(link.expire(10).unwrap().requests, vec![request]);
    assert_eq!(link.renew(5, 9), Err(Error::TimeRegression));
    assert!(link.expire(30).unwrap().link_lost);
    assert_eq!(link.peer_activity(31), Err(Error::Disconnected));
}
#[test]
fn renewal_cannot_revive_expired_lease_and_revocation_releases_everything() {
    let mut link = LinkState::new(LinkId::new([1; 16]).unwrap(), limits(), 5, 0).unwrap();
    assert_eq!(link.renew(51, 0), Err(Error::InvalidLease));
    let request = link.admit(20, 0).unwrap();
    assert_eq!(link.renew(5, 5), Err(Error::LeaseExpired));
    let loss = link.expire(5).unwrap();
    assert_eq!(loss.reason, Some(LossReason::LeaseExpired));
    assert_eq!(loss.requests, vec![request]);
    assert_eq!(link.usage(), Usage::default());
    let mut link = super_link();
    let request = link.admit(5, 0).unwrap();
    let loss = link.revoke(1).unwrap();
    assert_eq!(loss.reason, Some(LossReason::Revoked));
    assert_eq!(loss.requests, vec![request]);
    assert_eq!(link.admit(5, 1), Err(Error::Disconnected));
}
fn super_link() -> LinkState {
    link()
}
#[test]
fn deadline_overflow_fails_before_admission() {
    let mut link =
        LinkState::new(LinkId::new([1; 16]).unwrap(), limits(), 1, u64::MAX - 1).unwrap();
    assert_eq!(link.admit(1, u64::MAX - 1), Err(Error::TimeOverflow));
    assert_eq!(link.usage(), Usage::default());
}
#[test]
fn seeded_fault_schedule_matches_independent_queue_oracle_and_replays_exactly() {
    for seed in 0..64u64 {
        assert_eq!(run(seed), run(seed));
    }
}
fn run(seed: u64) -> u64 {
    let mut rng = seed + 1;
    let mut link = link();
    let mut epoch = 1u64;
    let mut requests: std::collections::BTreeMap<u64, (usize, u64)> =
        std::collections::BTreeMap::new();
    let mut next = 1;
    let mut last_peer = 0;
    let mut lease_deadline = 50;
    let mut connected = true;
    let mut digest = 0u64;
    for now in 0..1000u64 {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let expired = link.expire(now).unwrap();
        let dead = connected && (now >= lease_deadline || now - last_peer >= 30);
        let expected: Vec<_> = requests
            .iter()
            .filter(|(_, (_, deadline))| dead || now >= *deadline)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(
            expired
                .requests
                .iter()
                .map(|id| id.sequence())
                .collect::<Vec<_>>(),
            expected,
            "seed {seed}, time {now}"
        );
        assert_eq!(expired.link_lost, dead);
        for id in expected {
            requests.remove(&id);
            digest = digest.wrapping_mul(31).wrapping_add(id);
        }
        if dead {
            connected = false;
        }
        if !connected && rng.is_multiple_of(4) {
            epoch += 1;
            let mut bytes = [0; 16];
            bytes[..8].copy_from_slice(&epoch.to_be_bytes());
            link = LinkState::new(LinkId::new(bytes).unwrap(), limits(), 50, now).unwrap();
            assert!(requests.is_empty());
            next = 1;
            last_peer = now;
            lease_deadline = now + 50;
            connected = true;
        }
        match rng % 5 {
            0 if connected => {
                link.peer_activity(now).unwrap();
                last_peer = now;
            }
            1 if connected => {
                let bytes = (rng >> 8) as usize % 60 + 1;
                let used: usize = requests.values().map(|(bytes, _)| bytes).sum();
                let result = link.admit(bytes, now);
                if requests.len() < 3 && used + bytes <= 100 {
                    let id = result.unwrap();
                    assert_eq!(id.sequence(), next);
                    requests.insert(next, (bytes, now + 10));
                    next += 1;
                } else {
                    assert_eq!(result, Err(Error::Overloaded));
                }
            }
            2 if connected && !requests.is_empty() => {
                let (&id, _) = requests.first_key_value().unwrap();
                link.ack(RequestId::new(link.id(), id).unwrap(), now)
                    .unwrap();
                requests.remove(&id);
            }
            3 if connected => {
                let ttl = (rng >> 16) % 50 + 1;
                link.renew(ttl, now).unwrap();
                lease_deadline = now + ttl;
            }
            _ => {}
        }
        assert_eq!(
            link.usage(),
            Usage {
                pending_requests: requests.len(),
                queued_bytes: requests.values().map(|(bytes, _)| bytes).sum()
            }
        );
        digest = digest
            .wrapping_mul(31)
            .wrapping_add(link.usage().queued_bytes as u64)
            .wrapping_add(epoch);
    }
    digest
}
