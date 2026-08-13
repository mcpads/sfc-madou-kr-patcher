use super::text_to_json_convention;

#[test]
fn json_convention_preserves_text_after_multibyte_speaker() {
    let text = "\n[BOX:アルル] がおぉーー！！";

    assert_eq!(text_to_json_convention(text), "{BOX:アルル}がおぉーー！！");
}

#[test]
fn json_convention_preserves_controls_after_box_marker() {
    let text = "\n[BOX:NPC] こんにちは\nまたね▽|<CHOICE>";

    assert_eq!(
        text_to_json_convention(text),
        "{BOX:NPC}こんにちは{NL}またね{PAGE}{SEP}{CHOICE}"
    );
}
