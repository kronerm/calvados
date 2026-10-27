//! SmartRouting (0x10 out, 0x11 in): multi-device audio handoff. Payloads are
//! `[peer mac 6, reversed]` followed by a string-keyed TLV blob.

use crate::MacAddress;
use crate::update::News;

use super::frame::data_frame;

pub const OPCODE: u16 = 0x10;
pub const RESPONSE_OPCODE: u16 = 0x11;

/// A peer's routing frame, relayed by the pods. A peer taking the audio
/// sends three: [`Routing::PeerMedia`], [`Routing::ShowNearbyUi`], then
/// [`Routing::YieldRequest`] (our own [`hijack_sequence`] mirrors it).
///
/// [`hijack_sequence`]: crate::protocol::hijack_sequence
#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Routing {
    /// The peer's playback: `HostStreamingState` YES or NO.
    PeerMedia {
        streaming: bool,
    },
    /// The peer asks us to show the "audio moved" banner.
    ShowNearbyUi,
    /// A peer is taking audio; give up ownership.
    YieldRequest,
    /// The owner declined our hijack (e.g. it's on a call).
    HijackDeclined,
    /// Another calvados host's [`News`] ([`news`]).
    News(News),
    Other,
}

#[derive(Debug, Clone, Copy)]
pub struct RoutingResponse {
    /// The peer the response is about; lets us learn peer MACs.
    pub from: Option<MacAddress>,
    pub kind: Routing,
}

/// `[mac 6, reversed] [tlv…]`. Never fails.
pub fn parse_response(input: &[u8]) -> nom::IResult<&[u8], RoutingResponse> {
    let from = input
        .first_chunk::<6>()
        .map(|b| super::from_reversed(*b))
        .filter(|mac| !mac.is_nil());
    // `[len 2]`, then ours start with their magic.
    if let Some(news) = input.get(8..).and_then(News::decode) {
        return Ok((
            &[],
            RoutingResponse {
                from,
                kind: Routing::News(news),
            },
        ));
    }
    let find = |needle: &[u8]| input.windows(needle.len()).position(|w| w == needle);
    let contains = |needle: &[u8]| find(needle).is_some();
    // `HostStreamingState` `42` `YES` / `NO`.
    let streaming = find(b"HostStreamingState").map(|at| {
        let value = &input[at + b"HostStreamingState".len()..];
        value.get(1..4) == Some(b"YES")
    });
    let kind = match (
        streaming,
        contains(b"ShowNearbyUI"),
        contains(b"SetOwnershipToFalse"),
        contains(b"ReverseBannerTapped"),
    ) {
        (Some(streaming), ..) => Routing::PeerMedia { streaming },
        (None, true, ..) => Routing::ShowNearbyUi,
        (None, false, true, true) => Routing::HijackDeclined,
        (None, false, true, false) => Routing::YieldRequest,
        _ => Routing::Other,
    };
    Ok((&[], RoutingResponse { from, kind }))
}

/// A 0x10 frame to `target`, body zero-padded (or truncated) to `len`.
fn frame(target: MacAddress, len: usize, fields: &[&[u8]]) -> Vec<u8> {
    let mut body = [&super::to_reversed(target)[..]]
        .into_iter()
        .chain(fields.iter().copied())
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    body.resize(len, 0);
    data_frame(OPCODE, &body)
}

/// [`News`] for the calvados host `target`: `[mac] [len 2 LE]` and the
/// news, which starts with its magic where Apple's frames have their
/// dictionary (`01 e5 …`).
pub fn news(target: MacAddress, news: News) -> Vec<u8> {
    let news = news.encode();
    let len = u16::try_from(news.len()).expect("short news");
    let body = [&super::to_reversed(target)[..], &len.to_le_bytes(), &news].concat();
    data_frame(OPCODE, &body)
}

/// Ask the firmware to move audio from `target` to us.
pub fn hijack_request(target: MacAddress) -> Vec<u8> {
    frame(
        target,
        106,
        &[
            &[0x62, 0x00, 0x01, 0xE5, 0x4A],
            b"localscore",
            &[0x30, 0x64, 0x46],
            b"reason",
            &[0x48],
            b"Hijackv2",
            &[0x51],
            b"audioRoutingScore",
            &[0x31, 0x2D, 0x01, 0x5F],
            b"audioRoutingSetOwnershipToFalse",
            &[0x01, 0x4B],
            b"remotescore",
            &[0xA5],
        ],
    )
}

/// Tell the firmware about our streaming state.
pub fn media_information(local: MacAddress, target: MacAddress, streaming: bool) -> Vec<u8> {
    frame(
        target,
        138,
        &[
            &[0x82, 0x00, 0x01, 0xE5, 0x4A],
            b"PlayingApp",
            &[0x56],
            b"com.google.ios.youtube",
            &[0x52],
            b"HostStreamingState",
            &[0x42],
            if streaming { b"YES" } else { b"NO" },
            &[0x49],
            b"btAddress",
            &[0x51],
            local.to_hex_string().to_uppercase().as_bytes(),
            b"btName",
            &[0x43],
            b"Mac",
            &[0x58],
            b"otherDevice",
            b"AudioCategory",
            &[0x31, 0x2D, 0x01],
        ],
    )
}

/// Make `target` show the "audio moved" banner.
pub fn show_ui(target: MacAddress) -> Vec<u8> {
    frame(
        target,
        134,
        &[
            &[0x7E, 0x00, 0x01, 0xE6, 0x5B],
            b"SmartRoutingKeyShowNearbyUI",
            &[0x01, 0x4A],
            b"localscore",
            &[0x31, 0x2D, 0x01, 0x46],
            b"reasonHhijackv2",
            &[0x51],
            b"audioRoutingScore",
            &[0xA2, 0x5F],
            b"audioRoutingSetOwnershipToFalse",
            &[0x01, 0x4B],
            b"remotescore",
            &[0xA2],
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: &str = "02:11:22:33:44:55";
    const PEER: &str = "02:66:77:88:99:aa";

    #[test]
    fn builders_match_captured_bytes() {
        let (local, peer) = (LOCAL.parse().unwrap(), PEER.parse().unwrap());
        assert_eq!(
            hex::encode(hijack_request(peer)),
            "040004001000aa9988776602620001e54a6c6f63616c73636f7265306446726561736f6e4848696a61636b763251617564696f526f7574696e6753636f7265312d015f617564696f526f7574696e675365744f776e657273686970546f46616c7365014b72656d6f746573636f7265a5"
        );
        assert_eq!(
            hex::encode(media_information(local, peer, true)),
            "040004001000aa9988776602820001e54a506c6179696e6741707056636f6d2e676f6f676c652e696f732e796f757475626552486f737453747265616d696e67537461746542594553496274416464726573735130323a31313a32323a33333a34343a353562744e616d65434d6163586f74686572446576696365417564696f43617465676f7279312d010000000000"
        );
        assert_eq!(
            hex::encode(show_ui(peer)),
            "040004001000aa99887766027e0001e65b536d617274526f7574696e674b657953686f774e65617262795549014a6c6f63616c73636f7265312d0146726561736f6e4868696a61636b763251617564696f526f7574696e6753636f7265a25f617564696f526f7574696e675365744f776e657273686970546f46616c7365014b72656d6f746573636f7265a2"
        );
    }

    #[test]
    fn response_kinds() {
        let parse = |tail: &[u8]| {
            let body = [&[0xaa, 0x99, 0x88, 0x77, 0x66, 0x02][..], tail].concat();
            parse_response(&body).unwrap().1
        };
        let yield_req = parse(b"audioRoutingSetOwnershipToFalse");
        assert_eq!(yield_req.kind, Routing::YieldRequest);
        assert_eq!(yield_req.from, PEER.parse().ok());
        assert_eq!(
            parse(b"SetOwnershipToFalse ReverseBannerTapped").kind,
            Routing::HijackDeclined
        );
        assert_eq!(parse(b"whatever").kind, Routing::Other);
        // The three frames of a peer's takeover, as relayed to the laptop.
        let peer = PEER.parse().unwrap();
        let relayed = |frame: Vec<u8>| parse(&frame[12..]).kind;
        let local = LOCAL.parse().unwrap();
        assert_eq!(
            relayed(media_information(local, peer, true)),
            Routing::PeerMedia { streaming: true }
        );
        assert_eq!(
            relayed(media_information(local, peer, false)),
            Routing::PeerMedia { streaming: false }
        );
        assert_eq!(relayed(show_ui(peer)), Routing::ShowNearbyUi);
        assert_eq!(relayed(hijack_request(peer)), Routing::YieldRequest);
    }

    #[test]
    fn news_is_relayed_as_is() {
        use crate::aacp::Packet;
        use crate::protocol::parse_datagram;
        use crate::update::UpdateStatus;

        let peer = PEER.parse().unwrap();
        let news = News::Update(UpdateStatus::Transferring { percent: 40 });
        let sent = super::news(peer, news);
        assert!(hex::encode(&sent).starts_with("040004001000aa9988776602190063616c7661646f73"));
        // The pods relay it as 0x11, from the sender.
        let relayed = [&[0x04, 0x00, 0x04, 0x00, 0x11, 0x00][..], &sent[6..]].concat();
        let packets = parse_datagram(&[relayed.clone(), relayed].concat());
        assert_eq!(packets.len(), 2, "its length frames it");
        let Packet::RoutingResponse(response) = &packets[0] else {
            panic!("a routing response");
        };
        assert_eq!(response.kind, Routing::News(news));
        assert_eq!(response.from, Some(peer));
    }
}
