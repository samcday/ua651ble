use super::*;

const PACKET: &[u8] = &[
    0x1e, 120, 0, 80, 0, 93, 0, 0xe8, 7, 4, 5, 6, 7, 8, 66, 0, 2, 4, 0,
];

#[test]
fn measurement_and_status() {
    let reading = decode("AA:BB:CC:DD:EE:FF", PACKET).unwrap();
    assert_eq!(reading.systolic, Some(120.0));
    assert_eq!(reading.diastolic, Some(80.0));
    assert_eq!(reading.map, Some(93.0));
    assert_eq!(reading.pulse, Some(66.0));
    assert_eq!(reading.unit, "mmHg");
    assert_eq!(reading.user_id, Some(2));
    assert_eq!(reading.measured_at.as_deref(), Some("2024-04-05T06:07:08"));
    assert!(reading.to_string().ends_with("Status: irregular pulse"));
    assert_eq!(
        serde_json::to_value(&reading).unwrap()["status"]["irregular_pulse"],
        true
    );
}

#[test]
fn rejects_incomplete_and_extra_data() {
    for length in 0..PACKET.len() {
        assert!(decode("test", &PACKET[..length]).is_err());
    }
    let mut bytes = PACKET.to_vec();
    bytes.push(0);
    assert!(decode("test", &bytes).is_err());
}

#[test]
fn sfloat_numbers_and_special_values() {
    assert_eq!(sfloat(0xf4b5), Some(120.5));
    assert_eq!(sfloat(0xf098), Some(15.2));
    assert_eq!(sfloat(0x107b), Some(1230.0));
    assert_eq!(sfloat(0x0fff), Some(-1.0));
    assert_eq!(sfloat(0xffff), Some(-0.1));
    assert_eq!(sfloat(0x8001), Some(0.00000001));
    assert_eq!(sfloat(0), Some(0.0));
    for special in 0x07fe_u16..=0x0802 {
        assert_eq!(sfloat(special), None);
    }
}
