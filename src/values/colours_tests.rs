use super::*;
use crate::natives::{Native, NativeSet};
use crate::testing::Fixture;
use crate::values::sites::tests::with_params;

const MANIFEST: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n";
const CLIENT: &str = "res/client/main.lua";

fn fixture(text: &str, natives: Vec<Native>) -> Fixture {
    Fixture::new(&[("res/fxmanifest.lua", MANIFEST), (CLIENT, text)]).with_natives(natives)
}

fn ptfx() -> Native {
    let params = [("ptfxHandle", "int"), ("r", "float"), ("g", "float"), ("b", "float"), ("bLocalOnly", "BOOL")];
    with_params(NativeSet::Gta5, "SetParticleFxLoopedColour", "GRAPHICS", &params)
}

fn read(fx: &Fixture) -> Vec<Colour> {
    colours(&fx.cx(&fx.doc(CLIENT)))
}

fn spans(fx: &Fixture) -> Vec<String> {
    let doc = fx.doc(CLIENT);
    read(fx).iter().map(|c| doc.text()[c.start..c.end].to_owned()).collect()
}

fn bytes(rgba: [u8; 4]) -> [f32; 4] {
    rgba.map(|v| f32::from(v) / 255.0)
}

fn present(fx: &Fixture, colour: &Colour, rgba: [u8; 4]) -> Option<String> {
    presentation(&fx.cx(&fx.doc(CLIENT)), colour.start, colour.end, bytes(rgba))
}

fn pick(fx: &Fixture, nth: usize, rgba: [u8; 4]) -> String {
    let doc = fx.doc(CLIENT);
    let colour = read(fx).into_iter().nth(nth).expect("a colour");
    let text = present(fx, &colour, rgba).expect("a presentation");
    format!("{}{text}{}", &doc.text()[..colour.start], &doc.text()[colour.end..])
}

#[test]
fn draw_marker_rgba_literals_are_read_and_rewritten() {
    let fx = fixture(
        "DrawMarker(1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 255, 128, 0, 200, false, true, 2, false, nil, nil, false)",
        Vec::new(),
    );
    let found = read(&fx);
    assert_eq!(found.len(), 1, "one swatch per colour");
    assert_eq!(found[0].rgba, bytes([255, 128, 0, 200]));
    assert_eq!(spans(&fx), ["255, 128, 0, 200"]);
    assert_eq!(
        pick(&fx, 0, [10, 20, 30, 40]),
        "DrawMarker(1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 10, 20, 30, 40, false, true, 2, false, nil, nil, false)"
    );
}

#[test]
fn a_colour_with_a_variable_component_has_no_swatch_and_a_variable_alpha_is_kept() {
    assert_eq!(read(&fixture("DrawRect(0.5, 0.5, 0.1, 0.1, r, 0, 0, 255)", Vec::new())), []);
    let fx = fixture("DrawRect(0.5, 0.5, 0.1, 0.1, 0, 255, 0, alpha)", Vec::new());
    assert_eq!(read(&fx)[0].rgba, bytes([0, 255, 0, 255]));
    assert_eq!(spans(&fx), ["0, 255, 0"]);
    assert_eq!(pick(&fx, 0, [1, 2, 3, 4]), "DrawRect(0.5, 0.5, 0.1, 0.1, 1, 2, 3, alpha)");
}

#[test]
fn float_colour_components_are_read_and_written_as_floats() {
    let fx = fixture("SetParticleFxLoopedColour(fx, 1.0, 0.5, 0.0, false)", vec![ptfx()]);
    assert_eq!(read(&fx)[0].rgba, bytes([255, 128, 0, 255]));
    assert_eq!(pick(&fx, 0, [0, 51, 255, 255]), "SetParticleFxLoopedColour(fx, 0.0, 0.2, 1.0, false)");
    assert_eq!(format(128, true), "0.502");
    assert_eq!(format(128, false), "128");
    assert_eq!(format(255, true), "1.0");
    assert_eq!(format(0, true), "0.0");
}

#[test]
fn the_range_covers_every_component_and_keeps_what_separates_them() {
    let text = "DrawRect(0.5, 0.5, 0.1, 0.1,\n    255, -- red\n    --[[ green ]] 128,\n    0,\n    200)";
    let fx = fixture(text, Vec::new());
    assert_eq!(spans(&fx), ["255, -- red\n    --[[ green ]] 128,\n    0,\n    200"]);
    assert_eq!(
        pick(&fx, 0, [10, 20, 30, 40]),
        "DrawRect(0.5, 0.5, 0.1, 0.1,\n    10, -- red\n    --[[ green ]] 20,\n    30,\n    40)"
    );
}

#[test]
fn an_unchanged_alpha_literal_is_not_rewritten() {
    let fx = fixture("DrawRect(0.5, 0.5, 0.1, 0.1, 0xFF, 0, 0, 0xC8)", Vec::new());
    assert_eq!(read(&fx)[0].rgba, bytes([255, 0, 0, 200]));
    assert_eq!(pick(&fx, 0, [0, 0, 255, 200]), "DrawRect(0.5, 0.5, 0.1, 0.1, 0, 0, 255, 0xC8)");
    assert_eq!(pick(&fx, 0, [0, 0, 255, 199]), "DrawRect(0.5, 0.5, 0.1, 0.1, 0, 0, 255, 199)");
}

#[test]
fn every_channel_sent_as_a_float_rounds_back_to_the_chosen_byte() {
    let ints = fixture("DrawRect(0.5, 0.5, 0.1, 0.1, 1, 2, 3, 4)", Vec::new());
    let floats = fixture("SetParticleFxLoopedColour(fx, 0.1, 0.2, 0.3, false)", vec![ptfx()]);
    let (int_colour, float_colour) = (&read(&ints)[0], &read(&floats)[0]);
    for v in 0..=255u8 {
        assert_eq!(present(&ints, int_colour, [v, v, v, 7]), Some(format!("{v}, {v}, {v}, 7")));
        let f = format(v, true);
        assert_eq!(present(&floats, float_colour, [v, v, v, 255]), Some(format!("{f}, {f}, {f}")));
        let back = lua_number(&f).map(|x| (x * 255.0).round());
        assert_eq!(back, Some(f64::from(v)), "{f} reads back as {v}");
    }
}

#[test]
fn two_colours_on_one_line_each_get_their_own_range() {
    let fx = fixture(
        "DrawRect(0.5, 0.5, 0.1, 0.1, 255, 0, 0, 255) DrawRect(0.1, 0.1, 0.1, 0.1, 0, 0, 255, 128)",
        Vec::new(),
    );
    assert_eq!(spans(&fx), ["255, 0, 0, 255", "0, 0, 255, 128"]);
    assert_eq!(read(&fx)[1].rgba, bytes([0, 0, 255, 128]));
    assert_eq!(
        pick(&fx, 1, [9, 9, 9, 128]),
        "DrawRect(0.5, 0.5, 0.1, 0.1, 255, 0, 0, 255) DrawRect(0.1, 0.1, 0.1, 0.1, 9, 9, 9, 128)"
    );
}

#[test]
fn a_channel_out_of_range_or_not_a_literal_number_gives_no_colour() {
    for text in [
        "DrawRect(0.5, 0.5, 0.1, 0.1, 256, 0, 0, 255)",
        "DrawRect(0.5, 0.5, 0.1, 0.1, 255, 0, 0, 300)",
        "DrawRect(0.5, 0.5, 0.1, 0.1, -1, 0, 0, 255)",
        "DrawRect(0.5, 0.5, 0.1, 0.1, (255), 0, 0, 255)",
        "DrawRect(0.5, 0.5, 0.1, 0.1, 255, 0)",
        "drawRect(0.5, 0.5, 0.1, 0.1, 255, 0, 0, 255)",
        "local DrawRect = print\nDrawRect(0.5, 0.5, 0.1, 0.1, 255, 0, 0, 255)",
    ] {
        assert_eq!(read(&fixture(text, Vec::new())), [], "{text}");
    }
    assert_eq!(read(&fixture("SetParticleFxLoopedColour(fx, 2.0, 0.5, 0.0, false)", vec![ptfx()])), []);
    let missing_alpha = fixture("SetTextColour(1, 2, 3)", Vec::new());
    assert_eq!(read(&missing_alpha)[0].rgba, bytes([1, 2, 3, 255]));
}

#[test]
fn a_presentation_needs_a_colour_range_and_finite_channels() {
    let fx = fixture("SetTextColour(1, 2, 3, 4)", Vec::new());
    let colour = read(&fx).remove(0);
    let cx_doc = fx.doc(CLIENT);
    let cx = fx.cx(&cx_doc);
    assert_eq!(presentation(&cx, colour.start, colour.end - 1, bytes([0; 4])), None);
    assert_eq!(presentation(&cx, colour.start, colour.end, [f32::NAN, 0.0, 0.0, 0.0]), None);
    assert_eq!(presentation(&cx, colour.start, colour.end, [2.0, 0.0, 0.0, 0.0]), None);
}

#[test]
fn at_most_ten_thousand_colours_are_given() {
    let fx = fixture(&"SetTextColour(1, 2, 3, 4)\n".repeat(MAX_COLOURS + 1), Vec::new());
    let found = read(&fx);
    assert_eq!(found.len(), MAX_COLOURS);
    assert!(found.windows(2).all(|w| w[0].start < w[1].start));
}
