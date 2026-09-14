use morrow_network_codecs::{Codec, browser::Engine};

#[test]
fn browser_fixture_engine_checks_semantics_before_measurement() {
    for codec in [Codec::Json, Codec::Cbor, Codec::Protobuf] {
        let engine = Engine::new(codec).unwrap();
        assert_eq!(engine.len(), 46);
        for index in 0..engine.len() {
            let bytes = engine.encode(index).unwrap();
            assert!(engine.check(index, &bytes).unwrap());
            assert!(engine.decode(index, &[]).is_err());
            assert_eq!(engine.batch(index, 7, true).unwrap(), bytes.len() * 7);
            assert_eq!(engine.batch(index, 7, false).unwrap(), 7);
        }
        assert!(engine.encode(46).is_err());
        assert!(engine.batch(0, 0, true).is_err());
        assert!(engine.batch(0, 100_001, true).is_err());
    }
}

#[test]
fn production_protobuf_keeps_the_measured_fixture_wire_bytes() {
    use morrow_network_codecs::{Message, corpus, encode, protocol::binary};
    for (name, message) in corpus::fixtures() {
        let measured = encode(Codec::Protobuf, &message).unwrap();
        let production = match &message {
            Message::Client(value) => {
                let encoded = binary::encode_client(value).unwrap();
                assert_eq!(binary::decode_client(&measured).unwrap(), *value, "{name}");
                encoded
            }
            Message::Server(value) => {
                let encoded = binary::encode_server(value).unwrap();
                assert_eq!(binary::decode_server(&measured).unwrap(), *value, "{name}");
                encoded
            }
        };
        assert_eq!(production, measured, "{name}");
    }
}
