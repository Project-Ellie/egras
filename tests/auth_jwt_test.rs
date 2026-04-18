use egras::auth::jwt::{encode_access_token, decode_access_token, Claims};
use uuid::Uuid;

#[test]
fn encode_then_decode_roundtrip() {
    let secret = "a".repeat(64);
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    let token = encode_access_token(&secret, "egras", sub, org, 3600).unwrap();

    let claims: Claims = decode_access_token(&secret, "egras", &token).unwrap();
    assert_eq!(claims.sub, sub);
    assert_eq!(claims.org, org);
    assert_eq!(claims.iss, "egras");
    assert_eq!(claims.typ, "access");
    assert!(claims.exp > claims.iat);
}

#[test]
fn rejects_bad_signature() {
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    let token = encode_access_token(&"a".repeat(64), "egras", sub, org, 3600).unwrap();
    let err = decode_access_token(&"b".repeat(64), "egras", &token).expect_err("bad sig");
    let s = format!("{err:#}");
    assert!(s.contains("signature") || s.contains("Invalid"), "got: {s}");
}

#[test]
fn rejects_wrong_issuer() {
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    let token = encode_access_token(&"a".repeat(64), "egras", sub, org, 3600).unwrap();
    assert!(decode_access_token(&"a".repeat(64), "nope", &token).is_err());
}

#[test]
fn rejects_expired_token() {
    let sub = Uuid::now_v7();
    let org = Uuid::now_v7();
    // ttl = -10 means already expired
    let token = encode_access_token(&"a".repeat(64), "egras", sub, org, -10).unwrap();
    assert!(decode_access_token(&"a".repeat(64), "egras", &token).is_err());
}
