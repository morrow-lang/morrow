//! A local observation of configured ownership and currently authenticated peer streams.
use super::{escape, peer};
use std::fmt::Write;

pub(super) fn render(body: &mut String, cluster: Option<&peer::Observation>) {
    let Some(cluster) = cluster else {
        body.push_str("<section class=\"panel cluster-panel\" aria-label=\"Server connections\"><div class=section-heading><div><p class=eyebrow>SERVER CONNECTIONS</p><h2>Standalone</h2></div><span class=badge>One server</span></div><p>Rooms run on this server. No peer connections are configured.</p></section>");
        return;
    };
    let _ = write!(
        body,
        "<section class=\"panel cluster-panel\" aria-label=\"Server connections\"><div class=section-heading><div><p class=eyebrow>SERVER CONNECTIONS</p><h2>Node {}</h2></div><span class=badge>Fixed room ownership</span></div><dl class=cluster-grid>",
        escape(&cluster.node)
    );
    for (name, value) in [
        ("Configured nodes", cluster.configured_nodes.to_string()),
        ("Connected peers", cluster.connected_nodes.to_string()),
        (
            "Inbound streams",
            format!(
                "{} / {}",
                cluster.inbound_streams, cluster.stream_limit_per_direction
            ),
        ),
        (
            "Outbound streams",
            format!(
                "{} / {}",
                cluster.outbound_streams, cluster.stream_limit_per_direction
            ),
        ),
        ("Forwarded commands", cluster.forwarded_commands.to_string()),
        (
            "Rejected connections",
            cluster.rejected_connections.to_string(),
        ),
    ] {
        let _ = write!(
            body,
            "<div><dt>{name}</dt><dd>{}</dd></div>",
            escape(&value)
        );
    }
    body.push_str("</dl><p class=cluster-note>Configured nodes includes this server. Connected peers counts distinct authenticated remote nodes with an open stream; idle nodes may have no stream. Stream counts include connection setup. Forwarded commands counts outgoing transport writes, not confirmed changes.</p><p class=cluster-note>Unavailable owners keep their rooms unavailable. This connection snapshot does not establish cluster health, replication or automatic failover.</p></section>");
}
