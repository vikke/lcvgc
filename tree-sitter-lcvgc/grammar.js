module.exports = grammar({
  name: "lcvgc",

  extras: ($) => [/\s/, $.comment],

  rules: {
    source_file: ($) => repeat($._definition),

    _definition: ($) =>
      choice(
        $.device_definition,
        $.instrument_definition,
        $.kit_definition,
        $.clip_definition,
        $.scene_definition,
        $.session_definition,
        $.scale_command,
        $.tempo_command,
        $.var_definition,
        $.include_command,
        $.play_command,
        $.stop_command
      ),

    // ── Comments ──
    comment: ($) => token(seq("//", /[^\n]*/)),

    // ── Identifiers & literals ──
    identifier: ($) => /[a-zA-Z_][a-zA-Z0-9_]*/,
    number: ($) => /\d+(\.\d+)?/,
    string: ($) => seq('"', /[^"]*/, '"'),

    // ── Note names ──
    note_name: ($) => /[a-gA-G]#?/,

    // ── Device ──
    device_definition: ($) =>
      seq("device", field("name", $.identifier), "{", repeat($.device_property), "}"),

    device_property: ($) =>
      seq($.identifier, $.string),

    // ── Instrument ──
    instrument_definition: ($) =>
      seq(
        "instrument",
        field("name", $.identifier),
        "{",
        repeat($.instrument_property),
        "}"
      ),

    instrument_property: ($) =>
      choice(
        seq("device", $.identifier),
        seq("channel", $.number),
        seq("note", $.note_name, optional($.number)),
        seq("gate_normal", $.number),
        seq("gate_staccato", $.number),
        seq($.cc_definition)
      ),

    cc_definition: ($) => seq("cc", $.identifier, $.number),

    // ── Kit ──
    kit_definition: ($) =>
      seq("kit", field("name", $.identifier), "{", repeat($.kit_property), "}"),

    kit_property: ($) =>
      choice(
        seq("device", $.identifier),
        $.kit_instrument
      ),

    kit_instrument: ($) =>
      seq($.identifier, "{", repeat($.kit_instrument_property), "}"),

    kit_instrument_property: ($) =>
      choice(
        seq("channel", $.number),
        seq("note", $.note_name, optional($.number))
      ),

    // ── Clip ──
    clip_definition: ($) =>
      seq(
        "clip",
        field("name", $.identifier),
        repeat($.clip_option),
        "{",
        repeat($._clip_content),
        "}"
      ),

    clip_option: ($) =>
      seq("[", $.clip_option_key, $._clip_option_value, "]"),

    clip_option_key: ($) => choice("bars", "time", "scale"),

    _clip_option_value: ($) => choice($.number, $.identifier),

    _clip_content: ($) =>
      choice(
        $.pitch_note,
        $.chord_note,
        $.rest,
        $.drum_line,
        $.cc_automation,
        $.repeat_block,
        $.bar_jump
      ),

    // ── Pitch notes: c:3:8, c#:4:4 ──
    pitch_note: ($) =>
      seq(
        $.note_name,
        optional(seq(":", $.number)),
        optional(seq(":", $.number)),
        optional($.articulation)
      ),

    // ── Chords: cm7:4:2 ──
    chord_note: ($) =>
      seq(
        $.note_name,
        $.chord_quality,
        optional(seq(":", $.number)),
        optional(seq(":", $.number))
      ),

    chord_quality: ($) =>
      choice(
        "maj7", "maj", "min7", "min", "m7", "m",
        "dim7", "dim", "aug", "sus2", "sus4",
        "7", "9", "11", "13",
        "add9", "6"
      ),

    articulation: ($) => choice(".", "^", ">"),

    rest: ($) => "r",

    // ── Drum patterns ──
    drum_line: ($) => seq($.identifier, $.drum_pattern),

    drum_pattern: ($) => /[xX.o\-]+/,

    // ── CC automation ──
    cc_automation: ($) =>
      seq("cc", choice($.identifier, $.number), repeat($.cc_point)),

    cc_point: ($) => seq($.number, optional(seq(":", $.number))),

    // ── Repeat ──
    repeat_block: ($) =>
      seq("(", repeat($._clip_content), ")", "*", $.number),

    // ── Bar jump ──
    bar_jump: ($) => seq("|", $.number),

    // ── Scene ──
    scene_definition: ($) =>
      seq("scene", field("name", $.identifier), "{", repeat($.scene_entry), "}"),

    scene_entry: ($) =>
      seq(
        $.identifier,
        optional(seq(":", $.identifier)),
        repeat($.scene_entry_option)
      ),

    scene_entry_option: ($) =>
      seq("[", $.identifier, optional($.number), "]"),

    // ── Session ──
    session_definition: ($) =>
      seq(
        "session",
        field("name", $.identifier),
        "{",
        repeat($.session_entry),
        "}"
      ),

    session_entry: ($) =>
      seq(
        $.identifier,
        repeat($.session_entry_option)
      ),

    session_entry_option: ($) =>
      seq("[", choice("repeat", "loop"), optional($.number), "]"),

    // ── Scale ──
    scale_command: ($) =>
      seq("scale", $.note_name, optional($.number), $.scale_type),

    scale_type: ($) =>
      choice(
        "major",
        "minor",
        "dorian",
        "phrygian",
        "lydian",
        "mixolydian",
        "locrian",
        "harmonic_minor",
        "melodic_minor"
      ),

    // ── Tempo ──
    tempo_command: ($) => seq("tempo", optional(choice("+", "-")), $.number),

    // ── Var ──
    var_definition: ($) =>
      seq("var", field("name", $.identifier), "=", $._var_value),

    _var_value: ($) => choice($.number, $.string, $.identifier),

    // ── Include ──
    include_command: ($) => seq("include", $.string),

    // ── Play / Stop ──
    play_command: ($) =>
      seq(
        "play",
        optional("session"),
        $.identifier,
        repeat($.play_option)
      ),

    play_option: ($) =>
      seq("[", choice("repeat", "loop"), optional($.number), "]"),

    stop_command: ($) => seq("stop", optional($.identifier)),
  },
});
