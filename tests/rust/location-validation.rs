use super::*;
use serde_json::json;

#[test]
fn rejects_unrestorable_positions_and_accepts_reader_locations() {
    for location in [
        json!(null),
        json!([]),
        json!("page"),
        json!({"page":"<b>1</b>"}),
        json!({"page":0}),
        json!({"page":1.5}),
        json!({"chapter":-1}),
        json!({"page_index":9007199254740992_u64}),
        json!({"percent":101}),
        json!({"type":[]}),
        json!({"href":null}),
        json!({"color":123}),
        json!({"color":"x".repeat(33)}),
        json!({"quote":true}),
        json!({"quote":"x".repeat(4097)}),
        json!({"time":-1}),
        json!({"time":"120"}),
        json!({"duration":-5.0}),
        json!({"duration":"long"}),
        json!({"extra":"x".repeat(8192)}),
    ] {
        assert!(validate_location(&location).is_err(), "{location}");
    }
    for location in [
        json!({}),
        json!({"type":"page","page":1}),
        json!({"type":"chapter","chapter":0,"href":"OPS/chapter.xhtml"}),
        json!({"type":"comic","page_index":0,"page":1}),
        json!({"type":"percent","percent":37}),
        json!({"type":"chapter","chapter":1,"color":"yellow","quote":"trecho destacado"}),
        json!({"type":"audio","time":120.5,"duration":3600.0}),
        json!({"type":"video","time":300.0,"duration":1800.0}),
    ] {
        validate_location(&location).unwrap();
    }
    let note: SaveNote = serde_json::from_value(json!({"content":"note"})).unwrap();
    assert_eq!(note.location, json!({}));
}
