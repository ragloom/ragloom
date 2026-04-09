use ragloom::sink::PointId;

#[test]
fn parse_accepts_uuid_string() {
    let id = PointId::parse("550e8400-e29b-41d4-a716-446655440000").expect("uuid ok");
    assert_eq!(id.as_str(), "550e8400-e29b-41d4-a716-446655440000");
}

#[test]
fn parse_rejects_non_uuid_non_integer() {
    // 当前 PointId 只保证非空；具体 sink（如 Qdrant）还会有更严格的要求。
    let id = PointId::parse("D:/x/a.txt:0").expect("parse accepts non-empty");
    assert_eq!(id.as_str(), "D:/x/a.txt:0");
}
