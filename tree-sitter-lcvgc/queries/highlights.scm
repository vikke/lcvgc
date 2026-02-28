; Keywords - block definitions
"device" @keyword
"instrument" @keyword
"kit" @keyword
"clip" @keyword
"scene" @keyword
"session" @keyword

; Keywords - commands
"play" @keyword
"stop" @keyword
"tempo" @keyword
"scale" @keyword
"var" @keyword
"include" @keyword

; Keywords - properties
"channel" @keyword
"note" @keyword
"gate_normal" @keyword
"gate_staccato" @keyword
"cc" @keyword
"port" @keyword

; Keywords - options
"bars" @keyword
"time" @keyword
"repeat" @keyword
"loop" @keyword

; Scale types
(scale_type) @type

; Chord quality
(chord_quality) @type

; Note names
(note_name) @constant

; Drum patterns
(drum_pattern) @operator

; Articulation
(articulation) @operator

; Rest
(rest) @constant

; Strings
(string) @string

; Numbers
(number) @number

; Comments
(comment) @comment

; Identifiers - definition names
(device_definition name: (identifier) @function)
(instrument_definition name: (identifier) @function)
(kit_definition name: (identifier) @function)
(clip_definition name: (identifier) @function)
(scene_definition name: (identifier) @function)
(session_definition name: (identifier) @function)
(var_definition name: (identifier) @variable)

; General identifiers
(identifier) @variable

; Operators
"=" @operator
"+" @operator
"-" @operator
"*" @operator
"|" @punctuation.delimiter
":" @punctuation.delimiter

; Brackets
"{" @punctuation.bracket
"}" @punctuation.bracket
"[" @punctuation.bracket
"]" @punctuation.bracket
"(" @punctuation.bracket
")" @punctuation.bracket
