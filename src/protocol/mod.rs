
/*

All:
type: u8 Producer=1, Consumer=2
namespace_data: [u8]*[\n]
username_data: [u8]*[\n]
password_data: [u8]*[\n]

Producer: [
    op: u8, Produce=1, ModifyNamespace=4
    Produce:
        tags_data: [u8]* (,[u8])*[\n] (comma separated strings followed by newline)
        data_length: u32
        event_data: [u8]
    ModifyNamespace:
        key: [u8]*
        separator: [:]
        value: [u8]*[\n]
]...

Consumer:
    tags: [u8]* (,[u8])*[\n] (comma separated strings followed by newline)
*/

mod client_type;
mod producer;

use nom::{be_u8, be_u32, IResult};
pub use self::producer::{ProduceEvent, ModifyNamespace, ProducerMessage};
pub use self::client_type::ClientType;

#[derive(Debug, PartialEq)]
pub struct RequestHeader<'a> {
    namespace: &'a str,
    username: &'a str,
    password: &'a str,
}

named!{pub parse_str<&str>,
    map_res!(
        take_until_and_consume!("\n"),
        ::std::str::from_utf8
    )
}

named!{pub parse_header<RequestHeader>,
    chain!(
        _tag: tag!("FLO_INI\n") ~
        namespace: parse_str ~
        username: parse_str ~
        password: parse_str,
        || {
            RequestHeader {
                namespace: namespace,
                username: username,
                password: password,
            }
        }
    )
}

named!{pub parse_producer_event<ProduceEvent>,
    chain!(
        _tag: tag!("FLO_PRO\n") ~
        event_data: length_bytes!(be_u32),
        || {
            ProduceEvent {
                event_data: event_data,
            }
        }
    )
}

pub trait ClientProtocol {
    fn parse_header<'a>(&'a self, buffer: &'a [u8]) -> IResult<&'a [u8], RequestHeader>;
    fn parse_producer_event<'a>(&'a self, buffer: &'a [u8]) -> IResult<&'a [u8], ProduceEvent>;

}

pub struct ClientProtocolImpl;

impl ClientProtocol for ClientProtocolImpl {
    fn parse_header<'a>(&'a self, buffer: &'a [u8]) -> IResult<&'a [u8], RequestHeader> {
        parse_header(buffer)
    }

    fn parse_producer_event<'a>(&'a self, buffer: &'a [u8]) -> IResult<&'a [u8], ProduceEvent> {
        parse_producer_event(buffer)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use nom::IResult;

    #[test]
    fn parse_producer_event_parses_correct_event() {
        let mut input = Vec::new();
        input.extend_from_slice(b"FLO_PRO\n");
        input.extend_from_slice(&[0, 0, 0, 5]); // hacky way to set the length as a u32
        input.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

        let (remaining, result) = parse_producer_event(&input).unwrap();

        let expected = ProduceEvent {
            event_data: &[1, 2, 3, 4, 5]
        };
        assert_eq!(expected, result);
        assert_eq!(&[6, 7, 8], remaining);
    }


    #[test]
    fn parse_header_returns_incomplete_result_when_password_is_missing() {
        let mut input = Vec::new();
        input.extend_from_slice(b"FLO_INI\n");
        input.extend_from_slice(b"hello\n");
        input.extend_from_slice(b"world\n");

        let result = parse_header(&input);
        match result {
            IResult::Incomplete(_) => { }
            e @ _ => panic!("Expected Incomplete, got: {:?}", e)
        }
    }

    #[test]
    fn parse_header_parses_valid_header_with_no_remaining_bytes() {
        let mut input = Vec::new();
        input.extend_from_slice(b"FLO_INI\n");
        input.extend_from_slice(b"hello\n");
        input.extend_from_slice(b"usr\n");
        input.extend_from_slice(b"pass\n");
        let (remaining, result) = parse_header(&input).unwrap();

        let expected_header = RequestHeader {
            namespace: "hello",
            username: "usr",
            password: "pass",
        };
        assert_eq!(expected_header, result);
        assert!(remaining.is_empty());
    }

    #[test]
    fn parse_header_returns_error_result_when_namespace_contains_invalid_utf_characters() {
        let mut input = Vec::new();
        input.push(2);
        input.extend_from_slice(&vec![0, 0xC0, 0, 0, 2, 10]);
        input.extend_from_slice(b"usr\n");
        input.extend_from_slice(b"pass\n");
        let result = parse_header(&input);
        assert!(result.is_err());
    }


    #[test]
    fn parse_string_returns_empty_string_when_first_byte_is_a_newline() {
        let input = vec![10, 4, 5, 6, 7];
        let (remaining, result) = parse_str(&input).unwrap();
        assert_eq!("".to_owned(), result);
        assert_eq!(&vec![4, 5, 6, 7], &remaining);
    }

    #[test]
    fn parse_string_returns_string_with_given_length() {
        let mut input = Vec::new();
        let expected_string = "hello".to_owned();
        input.extend_from_slice(expected_string.as_bytes());
        input.push(0x0a);
        let extra_bytes = vec![7, 8, 9, 10];
        input.extend_from_slice(&extra_bytes);

        let (remaining, result) = parse_str(&input).unwrap();
        assert_eq!(expected_string, result);
        assert_eq!(&extra_bytes, &remaining);
    }
}

