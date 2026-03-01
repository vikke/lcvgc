# lcvgc (Live CV Gate Coder) DSL Specification


<!-- vim-markdown-toc GFM -->

* [Overview](#overview)
* [Startup Options](#startup-options)
* [1. Device Definition (device)](#1-device-definition-device)
* [2. Instrument Definition (instrument)](#2-instrument-definition-instrument)
    * [Default Gate Ratio Values](#default-gate-ratio-values)
* [3. Kit Definition (kit)](#3-kit-definition-kit)
* [4. Tempo (tempo)](#4-tempo-tempo)
* [4.1 Scale (scale)](#41-scale-scale)
* [5. File Splitting (include)](#5-file-splitting-include)
* [6. Variables (var)](#6-variables-var)
    * [6.1 Scope](#61-scope)
    * [6.2 Redefinition via eval](#62-redefinition-via-eval)
    * [6.3 Relationship with include](#63-relationship-with-include)
    * [6.4 Reserved Words](#64-reserved-words)
* [7. Clip Definition (clip)](#7-clip-definition-clip)
    * [7.1 bars Option](#71-bars-option)
    * [7.2 Time Signature (time)](#72-time-signature-time)
    * [7.3 Scale Specification (scale)](#73-scale-specification-scale)
    * [7.4 Pitched Instrument Notation](#74-pitched-instrument-notation)
        * [Parse Rules](#parse-rules)
    * [7.5 Shorthand Notation](#75-shorthand-notation)
        * [Single Note Shorthand](#single-note-shorthand)
        * [Chord Name Shorthand](#chord-name-shorthand)
        * [Mixing Single Notes and Chord Names](#mixing-single-notes-and-chord-names)
        * [Shorthand Within Chords](#shorthand-within-chords)
    * [7.6 Repetition](#76-repetition)
    * [7.7 Articulation (Gate Control)](#77-articulation-gate-control)
        * [Normal](#normal)
        * [Staccato `'`](#staccato-)
        * [Direct Gate Ratio Specification `gN`](#direct-gate-ratio-specification-gn)
        * [Combinations](#combinations)
        * [Notation Summary](#notation-summary)
        * [Gate Duration Calculation](#gate-duration-calculation)
        * [Retrigger Guarantee](#retrigger-guarantee)
        * [Minimum Gate Off Duration](#minimum-gate-off-duration)
    * [7.8 Multi-line Notation](#78-multi-line-notation)
    * [7.9 Bar Jump (`>N`)](#79-bar-jump-n)
    * [7.10 Chords (Bracket Notation)](#710-chords-bracket-notation)
    * [7.11 Chord Name Notation](#711-chord-name-notation)
    * [7.12 Arpeggio](#712-arpeggio)
    * [7.13 Drums (Step Sequencer Notation)](#713-drums-step-sequencer-notation)
        * [Hit Symbols](#hit-symbols)
        * [`|` Shortcut](#-shortcut)
        * [Repetition](#repetition)
        * [Probability Row](#probability-row)
    * [7.14 CC Automation](#714-cc-automation)
        * [Step Mode](#step-mode)
        * [Time-based Mode](#time-based-mode)
        * [Exponential Curve Interpolation](#exponential-curve-interpolation)
        * [Mixing Both Modes](#mixing-both-modes)
* [8. Scene Definition (scene)](#8-scene-definition-scene)
    * [8.1 Probability](#81-probability)
    * [8.2 Shuffle](#82-shuffle)
    * [8.3 Weighted Shuffle](#83-weighted-shuffle)
    * [8.4 Tempo Changes](#84-tempo-changes)
    * [8.5 Combinations](#85-combinations)
* [9. Session Definition (session)](#9-session-definition-session)
* [10. Playback Control](#10-playback-control)
    * [10.1 Scene Playback](#101-scene-playback)
    * [10.2 Session Playback](#102-session-playback)
    * [10.3 Stop](#103-stop)
* [11. Error Handling](#11-error-handling)
    * [11.1 eval Failure](#111-eval-failure)
    * [11.2 Undefined References](#112-undefined-references)
    * [11.3 Deletion Operations](#113-deletion-operations)
    * [11.4 Internal Engine Panics](#114-internal-engine-panics)
    * [11.5 MIDI Output Errors](#115-midi-output-errors)
    * [11.6 Neovim Disconnection](#116-neovim-disconnection)
* [12. Grammar Rules Summary](#12-grammar-rules-summary)

<!-- vim-markdown-toc -->

## Overview

lcvgc is a live coding engine that describes MIDI sequences in a text-based DSL and evaluates/plays them in real time. By selecting and evaluating (eval) arbitrary blocks from an editor, you can modify the playing music in real time.

It supports sequences for everything from modular synths (CV/Gate) to MIDI synths in general.

The file extension is `.cvg`.

---

## Startup Options

```
lcvgc [OPTIONS]

OPTIONS:
  --file <path>          DSL file (.cvg) to load on startup
  --port <N>             LSP server listen port. Default: 5555
  --midi-device <name>   MIDI output device name. Omit for system default
  --log-level <level>    Log level (error, warn, info, debug). Default: info
  --config <path>        Config file path. Default: ~/.config/lcvgc/config.toml
  -V, --version          Show version
  -h, --help             Show help
```

**Examples:**

```bash
# Start with default settings
$ lcvgc

# Load a file on startup
$ lcvgc --file my_song.cvg

# Specify port and MIDI device
$ lcvgc --port 7777 --midi-device "IAC Driver Bus 1"

# Start with debug logging
$ lcvgc --file live.cvg --log-level debug
```

When `--file` is specified, all blocks in the file are automatically eval'd on startup. This is useful for setting up an initial state without using `:LcvgcEval` from Neovim.

---

## 1. Device Definition (device)

Assigns a name to a MIDI port. The port name specifies the name recognized as a MIDI device by the OS.

```
device mutant_brain {
  port "Mutant Brain"
}

device volca_keys {
  port "volca keys"
}
```

---

## 2. Instrument Definition (instrument)

Assigns a name to a device + MIDI channel combination. Drum-type instruments specify a fixed note. The gate ratio controls the Note On to Note Off duration. CC mappings allow aliases for control change numbers.

```
instrument bass {
  device mutant_brain
  channel 1
  gate_normal 80           // Normal gate ratio (%). Default if omitted: 80
  gate_staccato 40         // Staccato gate ratio (%). Default if omitted: 40
  cc cutoff 74             // Assign alias "cutoff" to CC#74
  cc resonance 71          // CC#71
}

instrument lead {
  device mutant_brain
  channel 2
  gate_normal 75
  gate_staccato 30
  cc cutoff 74
  cc vibrato 1
}

instrument pad {
  device mutant_brain
  channel 3
  gate_normal 100          // 100 = legato (no Gate Off)
  gate_staccato 60
  cc pan 10
}

instrument keys {
  device mutant_brain
  channel 3
}
// gate_normal, gate_staccato omitted → default values apply

// Drum-type: has a fixed note
instrument bd {
  device mutant_brain
  channel 10
  note c2
  gate_normal 50           // Gate control is available for drums too
  gate_staccato 20
}

// Modular synth algorithm switching, etc.
instrument mod_osc {
  device mutant_brain
  channel 4
  cc algorithm 12          // Module-specific CC number
  cc waveform 14
}
```

### Default Gate Ratio Values

| Parameter | Default Value |
|-----------|---------------|
| `gate_normal` | 80 |
| `gate_staccato` | 40 |

---

## 3. Kit Definition (kit)

Defines drum-type instruments as a group. The device is specified at the kit level. Each instrument can have `gate_normal` and `gate_staccato` specified (defaults apply if omitted).

```
kit tr808 {
  device mutant_brain
  bd    { channel 10, note c2, gate_normal 50, gate_staccato 20 }
  snare { channel 10, note d2 }
  hh    { channel 10, note f#2, gate_normal 30, gate_staccato 10 }
  oh    { channel 10, note a#2, gate_normal 80 }
  clap  { channel 10, note d#2 }
}
```

---

## 4. Tempo (tempo)

Set globally. Can be eval'd independently.

```
// Set with a literal value
tempo 120

// Change immediately (just eval it)
tempo 140
```

Tempo changes can be specified within a scene.

```
// +5 BPM per loop
scene buildup {
  drums_a
  bass_a
  tempo +5
}

// Reset to a fixed value with a literal
scene drop {
  drums_a
  bass_a
  tempo 120
}
```

---

## 4.1 Scale (scale)

Set globally. Can be eval'd independently. Can be overridden with `[scale ...]` in a clip. The scale specification does not affect playback behavior; it serves as hint information for LSP completion.

```
// Set globally
scale c minor

// Change immediately (just eval it)
scale d dorian
```

To override at the clip level:

```
scale c minor

clip bass_a [bars 1] {
  // The global scale (c minor) applies
  bass c:3:8 d eb f::4 g::2
}

clip lead_a [bars 1] [scale d dorian] {
  // Overridden at the clip level → d dorian applies
  lead d:3:4 e f g
}
```

If `[scale ...]` is not specified on a clip, the global scale applies. If the global scale is also unset, the LSP provides only generic note name and chord name completions.

---

## 5. File Splitting (include)

Loads another `.cvg` file by relative path. Circular includes result in a parse error. If the same file is included more than once, subsequent includes are silently skipped (the engine tracks loaded paths).

```
include "./setup.cvg"
include "./clips/drums.cvg"
include "./clips/bass.cvg"
```

```
// setup.cvg
var dev = mutant_brain

// drums.cvg
include "./setup.cvg"       // First time: loaded

// song.cvg
include "./setup.cvg"       // Loaded
include "./drums.cvg"       // The include "setup.cvg" inside drums.cvg is skipped
```

The LSP provides file path completion.

---

## 6. Variables (var)

Define variables with `var name = value`. Reference them by writing the name directly without `$`. The parser first looks for an identifier at the value position as a variable in the current scope; if found, it is expanded; if not, it is treated as a literal.

```
// Global variables
var dev = mutant_brain
var default_gate = 80
var bass_ch = 1
var cutoff_cc = 74

instrument bass {
  device dev                    // Variable dev → mutant_brain
  channel bass_ch               // Variable bass_ch → 1
  gate_normal default_gate      // Variable default_gate → 80
  cc cutoff cutoff_cc           // Variable cutoff_cc → 74
}

instrument lead {
  device mutant_brain           // Writing directly without a variable is also fine
  channel 2
}
```

### 6.1 Scope

Two-level scope: global (top-level) and block (inside `{}`). The inner scope takes priority (shadowing).

```
var ch = 1

instrument bass {
  var ch = 3                    // Different value inside the block
  channel ch                    // → 3
}

instrument lead {
  channel ch                    // → 1 (global)
}
```

### 6.2 Redefinition via eval

Re-evaluating a global variable changes its value. However, blocks that have already been eval'd are not affected (the change takes effect the next time that block is eval'd).

```
var dev = mutant_brain
// eval bass → uses mutant_brain

var dev = keystep
// re-eval bass → uses keystep
```

### 6.3 Relationship with include

Global variables from included files are merged into the caller. When names conflict, the one eval'd later wins.

```
// config.cvg
var dev = mutant_brain
var default_gate = 80

// song.cvg
include "./config.cvg"          // dev, default_gate become available
var default_gate = 90           // Override

instrument bass {
  device dev                    // → mutant_brain
  gate_normal default_gate      // → 90 (overridden value)
}
```

### 6.4 Reserved Words

The following keywords cannot be used as variable names:

`device`, `instrument`, `kit`, `clip`, `scene`, `session`, `include`, `tempo`, `play`, `stop`, `var`, `port`, `channel`, `note`, `gate_normal`, `gate_staccato`, `cc`, `use`, `resolution`, `arp`, `bars`, `time`, `scale`, `repeat`, `loop`

---

## 7. Clip Definition (clip)

The unit of a playback pattern. Eval'ing a clip with the same name overwrites it, and clips used by a currently playing scene switch to the new content at the start of the next loop.

### 7.1 bars Option

```
// bars specified: fit to N bars
// If too short, pad the end with rests
// If overflow, truncate to N bars worth of length (warning displayed, not an error)
clip bass_a [bars 1] {
  bass c:3:8 c:3:8 eb:3:8 f:3:4 g:3:2
}

// bars omitted: loop based on the total duration of notes in the clip
// Playing clips of different lengths simultaneously creates polyrhythms
clip bass_poly {
  bass c:3:4 eb:3:4 f:3:4
}
```

### 7.2 Time Signature (time)

A time signature can be specified per clip. Default if omitted is 4/4.

```
// 3/4 time
clip waltz_bass [bars 2] [time 3/4] {
  bass c:3:4 e g
  bass f:3:4 a c
}

// 4/4 (default, can be omitted)
clip drums_a [bars 1] {
  use tr808
  resolution 16
  bd x|x|x|x          // 16 steps = 4 beats
}

// 3/4 drums
clip drums_waltz [bars 1] [time 3/4] {
  use tr808
  resolution 16
  bd x|x|x             // 12 steps = 3 beats
}
```

### 7.3 Scale Specification (scale)

Specifying a scale on a clip causes the LSP to offer diatonic chords and progression candidates for that scale in completions. The scale specification does not affect playback behavior; it serves as hint information for LSP completion.

```
// Scale specification
clip chords_a [bars 4] [scale c minor] {
  keys cm7:4:2       // LSP suggests next chord candidates:
                     //   fm7 (IVm7), gm7 (Vm7), g7 (V7),
                     //   ebM7 (bIII), abM7 (bVI), bb7 (bVII), dm7b5 (IIm7b5)
  keys fm7:3:2
  keys g7:3:2
  keys cm7:4:1
}

// Major scale
clip chords_b [bars 4] [scale g major] {
  keys gM7:4:2       // I → candidates: am7(II), bm7(III), cM7(IV), d7(V), em7(VI)
  keys cM7:4:2
  keys d7:3:2
  keys gM7:4:1
}

// Modes can also be specified
clip chords_c [bars 4] [scale d dorian] {
  keys dm7:4:2
  keys g7:3:2
  keys em7:3:2
  keys dm7:4:1
}
```

Progressive LSP completion behavior:

- After `[scale ` → root note candidates: `c`, `c#`, `db`, `d`, ... `b`
- After `[scale c ` → scale type candidates: `major`, `minor`, `harmonic_minor`, `melodic_minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`
- At a chord-writing position inside a clip with a scale specified → all diatonic chords for that scale
- When there is a preceding chord → next chord candidates based on the progression table (with degree information)

Supported scale types:

| Scale | Diatonic Chord Examples (key=c) |
|-------|------|
| major | cM7, dm7, em7, fM7, g7, am7, bm7b5 |
| minor (natural) | cm7, dm7b5, ebM7, fm7, gm7, abM7, bb7 |
| harmonic_minor | cmM7, dm7b5, ebM7#5, fm7, g7, abM7, bdim7 |
| melodic_minor | cmM7, dm7, ebM7#5, f7, g7, am7b5, bm7b5 |
| dorian | cm7, dm7, ebM7, f7, gm7, am7b5, bbM7 |
| mixolydian | c7, dm7, em7b5, fM7, gm7, am7, bbM7 |

The `[scale ...]` on a clip is optional. If omitted, the global `scale` setting (see Section 4.1) applies. If the global scale is also unset, the LSP provides only generic note name and chord name completions.

### 7.4 Pitched Instrument Notation

Format: `instrument_name note_or_chord_name[:octave][:duration] ...`

Both single notes and chord names use a unified 3-section format with `:` separators.

```
clip bass_a [bars 1] {
  // Full notation (3 sections)
  bass c:3:8 c:3:8 eb:3:8 f:3:4 g:3:2

  // Shorthand: octave and duration carry over from the previous value
  // Defaults at the start of a clip are o4, :4
  bass c:3:8 c eb f::4 g::2
  //   c:3:8 → o3, :8 are set
  //   c     → carries over o3, :8
  //   eb    → carries over o3, :8
  //   f::4  → carries over o3, changes only duration to :4 (:: omits octave)
  //   g::2  → carries over o3, changes only duration to :2
}

clip lead_a [bars 1] {
  lead eb:5:4 d::8 c bb:4:2
  //   eb:5:4 → o5, :4
  //   d::8   → carries over o5, changes to :8
  //   c      → o5, :8
  //   bb:4:2 → o4, :2
}
```

- Note names: `c c# db d d# eb e f f# gb g g# ab a a# bb b` (all lowercase)
- Chord names: `cm7`, `fM7`, `g7`, etc. (see Section 7.11 for the suffix list)
- Octave: `0-9` — specified with `:` separator. Omission carries over the previous value
- Duration: `1`=whole note, `2`=half note, `4`=quarter, `8`=eighth, `16`=sixteenth; dotted notes append `.` like `4.` or `8.`. Omission carries over the previous value
- Rest: `r[:duration]` (duration carries over if omitted)
- Staccato: append `'` to the end of a note → `gate_staccato` is applied
- Direct gate specification: append `gN` to the end of a note → plays with gate ratio N%

#### Parse Rules

Common parse rules for both single notes and chord names.

| Notation | Octave | Duration | Description |
|----------|--------|----------|-------------|
| `c` / `cm7` | carry over | carry over | Both omitted |
| `c:3` / `cm7:4` | 3 / 4 | carry over | Octave only specified |
| `c:3:8` / `cm7:4:2` | 3 / 4 | eighth / half | Full notation |
| `c::8` / `cm7::2` | carry over | eighth / half | Octave omitted, duration only changed |

### 7.5 Shorthand Notation

Octave and duration carry over from the previous value. Defaults at the start of a clip are o4, :4. This carry-over is maintained across lines.

#### Single Note Shorthand

```
clip bass_a [bars 2] {
  bass c:3:8 c eb f::4 g::2
  //   c:3:8 → o3, :8
  //   c     → o3, :8 (both carried over)
  //   eb    → o3, :8
  //   f::4  → carries over o3, changes to :4 (:: omits octave)
  //   g::2  → carries over o3, changes to :2

  // The second line also carries over the state from the end of the previous line (o3, :2)
  bass ab::8 g f eb::4 c::2
}
```

#### Chord Name Shorthand

The parse rules are the same for chord names. `::` for octave omission + duration change works the same way.

```
clip chords_a [bars 4] {
  keys cm7:4:2       // o4, :2
  keys fm7::1        // carries over o4, changes to :1 (:: omits octave)
  keys g7            // o4, :1 both carried over
  keys cm7:3:4       // changes to o3, changes to :4
}
```

#### Mixing Single Notes and Chord Names

Even when mixing single notes and chord names within the same clip, octave and duration carry over consistently.

```
clip mixed_a [bars 2] {
  keys cm7:4:2                   // o4, :2
  keys [f:3 a c eb]:2            // o3 (explicit), :2
  keys bbM7::1                   // carries over o3, :1 (:: omits octave)
}
```

#### Shorthand Within Chords

Within chords (bracket notation), the first note's octave becomes the reference, and subsequent ones can be omitted.

```
keys [c:4 eb g bb]:2         // c:4 establishes o4; eb, g, bb are o4
keys [f:3 a c eb]:2          // f:3 sets o3; a, c, eb are o3
keys [bb:3 d:4 f a]:1        // Octave crossings must be explicit
```

### 7.6 Repetition

`()*N` repeats a phrase. This is the same notation shared with the drum step sequencer notation.

```
clip bass_a [bars 4] {
  // Repeat the entire phrase 4 times
  bass (c:3:8 c eb f::4 g::2)*4

  // Repeat only a part
  bass c:3:8 (c eb)*3 f::4 g::2
}

clip chords_a [bars 4] {
  // Repeat a chord progression
  keys (cm7:4:2 fm7::1)*2
}
```

Octave/duration carry-over within repetitions does not reset to the beginning on each iteration; it carries over the state from the end of the previous iteration.

### 7.7 Articulation (Gate Control)

Controls the gate duration (Note On to Note Off period) of notes through articulation.

#### Normal

Unmodified notes have the instrument's `gate_normal` applied.

```
clip bass_a [bars 1] {
  bass c:3:8 c eb f::4 g::2
  // → Each note's duration × 80% (bass's gate_normal) is the Gate On period
}
```

#### Staccato `'`

Appending `'` to the end of a note applies `gate_staccato`.

```
clip bass_stac [bars 1] {
  bass c:3:8' c' eb' f::4' g::2
  // → Each note's duration × 40% (bass's gate_staccato) is the Gate On period
}
```

#### Direct Gate Ratio Specification `gN`

To change the gate ratio for a specific note, specify the percentage directly with `gN`.

```
clip bass_mix [bars 1] {
  bass c:3:8 d eg95 f::4 g::2
  // → Only e is 95%, others use gate_normal (80%)
}
```

#### Combinations

Dotted note + staccato and dotted note + direct gate specification are also possible.

```
clip bass_combo [bars 1] {
  bass c:3:4.' d:8              // Dotted quarter + staccato
  bass e:3:4.g30 f:8            // Dotted quarter + Gate 30%
}
```

#### Notation Summary

| Notation | Meaning | Example |
|----------|---------|---------|
| `c:3:4` | Normal (gate_normal applied) | Gate On = duration × 80% |
| `c:3:4'` | Staccato (gate_staccato applied) | Gate On = duration × 40% |
| `c:3:4.` | Dotted note (1.5× duration, gate_normal applied) | Gate On = dotted duration × 80% |
| `c:3:4.'` | Dotted + staccato | Gate On = dotted duration × 40% |
| `c:3:4g95` | Direct gate ratio specification (95%) | Gate On = duration × 95% |
| `c:3:4.g30` | Dotted + direct gate ratio specification (30%) | Gate On = dotted duration × 30% |

#### Gate Duration Calculation

```
gate_duration = note_duration × (gate_percent / 100)
rest_duration = note_duration - gate_duration
```

Example: At BPM 120, a quarter note (500ms) with gate_normal: 80 results in Gate On: 400ms, Gate Off: 100ms.

#### Retrigger Guarantee

By providing a Gate Off period, consecutive notes are guaranteed to retrigger the EG (envelope generator) from the Attack phase each time.

With `gate_normal: 100` (legato), there is no Gate Off period, so retrigger behavior depends on the synth's retrigger settings.

#### Minimum Gate Off Duration

If the calculated Gate Off period from the gate ratio is less than 5ms, a minimum of 5ms Gate Off is enforced (to guarantee retriggering). However, this restriction does not apply when `gate_normal: 100` (intentional legato).

### 7.8 Multi-line Notation

When consecutive lines with the same instrument name appear within a clip, they are concatenated as a continuation. This allows long clips to be split for readability. Octave/duration carry-over is maintained across lines. How many bars per line is up to the writer.

```
// 4 bars as 4 lines, one bar each
clip bass_a [bars 4] {
  bass c:3:8 c eb f::4 g::2
  bass ab:3:8 g f eb::4 c::2
  bass c:3:4 eb f g
  bass ab:3:2 g::2
}

// 4 bars as 2 lines, two bars each
clip bass_b [bars 4] {
  bass c:3:8 c eb f::4 g::2 ab:3:8 g f eb::4 c::2
  bass c:3:4 eb f g ab:3:2 g::2
}

// Writing everything on a single line is also fine
clip bass_c [bars 4] {
  bass c:3:8 c eb f::4 g::2 ab:3:8 g f eb::4 c::2 c:3:4 eb f g ab:3:2 g::2
}
```

The same applies to drums. Lines with the same instrument name are concatenated. Probability rows correspond only to the hit row immediately above them.

```
clip drums_a [bars 2] {
  use tr808
  resolution 16

  bd    x|x|x|x
        ..5...7.
  bd    x.x.|x|x.x.|x
        ....3.......5.

  hh    x.o.x.o.x.o.x.o.
        ..3...5...3...5.
  hh    x.o.x.o.X.o.x.o.
        ..5...7.....3...
}
```

### 7.9 Bar Jump (`>N`)

`>N` forcibly moves the current position to the beginning of bar N (1-based). Useful when bar calculations get off during live coding.

```
clip bass_a [bars 4] {
  // >N forcibly moves to the beginning of the specified bar
  bass c:3:1 d:3:1 >3 e:3:4 f:3:4 g:3:4 a:3:4 >4 g:3:1
  //   ^^^^^^^^^^^^     bars 1-2
  //              >3  jump to the beginning of bar 3
  //                 ^^^^^^^^^^^^^^^^^^^^^^^^ bar 3
  //                                      >4 jump to the beginning of bar 4
  //                                         ^^^^ bar 4
}
```

Rules:

- `>N` forcibly moves the current position to the beginning of bar N (1-based)
- If the current position is before bar N → pad with rests
- If the current position is past bar N → truncate the excess
- `>N` outside the bars range is a parse error (e.g., `>5` with `[bars 4]`)

Can also be used with the drum step sequencer notation. It is a different symbol from `|` (beat-head shortcut), so there is no confusion.

```
clip drums_a [bars 4] {
  use tr808
  resolution 16

  bd    x|x|x|x >2 x.x.|x|x.x.|x >3 x|x|x|x >4 x.x.x.x.x.x.x.x.
  snare |x||x   >2 |x||X         >3 |x||x   >4 |x|x.x.X...
}
```

### 7.10 Chords (Bracket Notation)

Enclosing notes in square brackets makes them sound simultaneously. Multiple Note On messages are sent on the same MIDI channel.

```
clip chords_a [bars 2] {
  keys [c:4 eb g bb]:2         // The first c:4 establishes o4; subsequent notes can omit it
  keys [f:3 a c eb]:2          // f:3 sets o3
  keys [bb:3 d:4 f a]:1        // Octave crossings must be explicit
}

// Two notes are also fine
clip fifths [bars 1] {
  keys [c:3 g:3]:2
  keys [f:3 c:4]:2
}
```

### 7.11 Chord Name Notation

Format: `instrument_name chord_name:octave:duration`

```
clip chords_named [bars 2] {
  keys cm7:4:2
  keys f7:3:2
  keys bbM7:3:1              // M7 = alias for Maj7
}

// Both Maj and M can be used
clip chords_alias [bars 2] {
  keys cMaj7:4:2             // Maj7
  keys cM7:4:2               // M7 (same meaning)
}
```

Chord name suffixes:

| Suffix | Alias | Meaning |
|--------|-------|---------|
| `M` | `Maj` | Major |
| `M7` | `Maj7` | Major seventh |
| `m` | — | Minor |
| `m7` | — | Minor seventh |
| `7` | — | Dominant seventh |
| `dim` | — | Diminished |
| `dim7` | — | Diminished seventh |
| `aug` | — | Augmented |
| `m7b5` | — | Half-diminished |
| `mM7` | `mMaj7` | Minor-major seventh |
| `sus4` | — | Suspended fourth |
| `sus2` | — | Suspended second |
| `6` | — | Sixth |
| `m6` | — | Minor sixth |
| `9` | — | Ninth |
| `m9` | — | Minor ninth |
| `add9` | — | Add nine |
| `13` | — | Thirteenth |
| `m13` | — | Minor thirteenth |

`/` is reserved for future slash chords (chord inversions with a specified bass note).

Bracket notation and chord name notation can be mixed.

```
clip chords_mixed [bars 2] {
  keys cm7:4:2
  keys [f:3 a:3 c:4 eb:4]:2    // Use individual note specification where specific voicing is desired
  keys bbM7:3:1
}
```

### 7.12 Arpeggio

Append `arp(direction, note_resolution)` after a chord.

```
clip arp_a [bars 1] {
  keys [c:4 eb:4 g:4 bb:4]:1 arp(up, 16)      // Ascending, 16th note intervals
}

clip arp_b [bars 1] {
  keys [c:4 eb:4 g:4 bb:4]:1 arp(down, 16)    // Descending
}

clip arp_c [bars 1] {
  keys [c:4 eb:4 g:4 bb:4]:1 arp(random, 8)   // Random, 8th note intervals
}

clip arp_d [bars 2] {
  keys cm7:4:1 arp(updown, 16)             // Up then down
}
```

- Direction: `up`, `down`, `updown`, `random`
- Note resolution: `4`, `8`, `16`, etc. (interval between each note onset)

### 7.13 Drums (Step Sequencer Notation)

Use `use` to specify a kit and `resolution` to set the note resolution per character.

```
clip drums_a [bars 1] {
  use tr808
  resolution 16          // 1 character = 16th note

  bd    x|x|x|x
  snare |x||x
  hh    x.o.x.o.x.o.x.o.
}
```

#### Hit Symbols

| Symbol | Meaning | MIDI Velocity |
|--------|---------|---------------|
| `x` | Normal hit | 100 |
| `X` | Accent | 127 |
| `o` | Ghost note | 40 |
| `.` | Rest | - |

#### `|` Shortcut

`|` fills from the current position to the next beat boundary (every 4 characters at resolution 16) with rests `.`.

```
bd    x|x|x|x
// Expands to: x...x...x...x...

snare |x||x
// Expands to: ....x.......x...
```

- A leading `|` makes the first beat entirely rests
- Consecutive `||` skips an entire beat

#### Repetition

`()*N` repeats a step pattern. This is the same notation shared with pitched instrument repetition (Section 7.6).

```
hh    (x.x.)*4              // Repeat x.x. four times
hh    (x.o.)*3 xxxx         // Change only the last beat
```

#### Probability Row

A per-step triggering probability can be written directly below a hit row (optional).

```
clip drums_a [bars 1] {
  use tr808
  resolution 16

  hh    x.o.x.o.x.o.x.o.
  // Probability: thin out ghost notes for a random feel
        ..5...7...3...5.
}
```

- Digits `1`-`9` = 10%-90%
- `.` or space = 100% (can be omitted)
- `0` = 0% (effectively muted)
- Digits at positions without hits are ignored
- If the probability row is omitted, everything is 100%
- The probability check is performed on every loop iteration

### 7.14 CC Automation

Uses CC aliases defined on an instrument to send MIDI Control Change messages within a clip. There are two modes: step mode and time-based + interpolation mode.

#### Step Mode

Shares the drum `resolution`. Values are 0-127 (decimal).

```
clip bass_a [bars 1] {
  resolution 16
  bass c:3:8 c eb f::4 g::2

  // Specify values in 16 steps
  bass.cutoff    0 10 20 30 40 50 60 70 80 90 100 110 120 127 127 127
  bass.resonance 40 40 40 40 60 60 60 60 80 80 80 80 127 127 127 127
}
```

Step mode cannot be used in clips with only pitched instruments and no resolution specified (use time-based mode instead).

#### Time-based Mode

Send CC values at arbitrary timings with `value@bar.beat`.

```
clip bass_b [bars 4] {
  bass c:3:8 c eb f::4 g::2

  // Point specification (value changes immediately)
  bass.cutoff 0@1.1 64@2.1 127@3.1 64@4.1

  // Linear interpolation: connect with - (intermediate values are auto-generated)
  bass.cutoff 0@1.1-127@3.1 64@4.1

  // Algorithm switching: change abruptly at the start of bar 2
  mod_osc.algorithm 0@1.1 64@2.1 127@3.1
}
```

Connecting with `-` linearly interpolates between two points, progressively sending CC messages. The engine automatically determines the interpolation send interval.

#### Exponential Curve Interpolation

Use `-exp` instead of `-` for exponential curve interpolation. Suitable for parameters that change logarithmically, such as filter cutoff.

```
clip bass_c [bars 4] {
  // Linear interpolation
  bass.cutoff 0@1.1-127@4.4

  // Exponential curve (rises slowly then shoots up at the end)
  bass.cutoff 0@1.1-exp127@4.4
}
```

#### Mixing Both Modes

Within the same clip, step mode and time-based mode can be used on different CC parameters. Mixing both modes on the same CC parameter is not allowed.

```
clip bass_mix [bars 2] {
  resolution 16
  bass c:3:8 c eb f::4 g::2

  // cutoff uses step mode
  bass.cutoff 0 10 20 30 40 50 60 70 80 90 100 110 120 127 127 127

  // pan uses time-based mode
  pad.pan 0@1.1-127@2.4
}
```

---

## 8. Scene Definition (scene)

Defines a combination of clips to play simultaneously.

```
scene intro {
  drums_a
  bass_a
}

scene verse {
  drums_a
  bass_a
  lead_a
}
```

### 8.1 Probability

Appending a digit (1-9) after a clip name specifies the triggering probability. Evaluated on each loop.

```
scene verse {
  drums_a
  bass_a
  lead_a 7                   // Plays with 70% probability
  chords_a 5                 // 50%
}
```

- `1`-`9` = 10%-90%
- Omitted = 100%

### 8.2 Shuffle

Separating multiple clip candidates with `|` causes one to be randomly chosen per loop.

```
scene chorus {
  drums_a | drums_funk       // One or the other plays each loop
  bass_a
  lead_a
  chords_a | chords_open     // Chords also change each time
}
```

### 8.3 Weighted Shuffle

Specify weights with `*N`.

```
scene verse_v2 {
  drums_a*3 | drums_funk     // drums_a 75%, drums_funk 25%
  bass_a
}
```

### 8.4 Tempo Changes

Tempo changes can be specified within a scene.

```
// +5 BPM per loop
scene buildup {
  drums_a
  bass_a
  tempo +5
}

// Reset to a fixed value with a literal
scene drop {
  drums_a
  bass_a
  tempo 120
}
```

### 8.5 Combinations

Probability, shuffle, and tempo changes can be combined.

```
scene breakdown {
  drums_a | drums_poly
  bass_a 6                                // Plays with 60% probability
  arp_a | arp_b | arp_c 8                 // Random selection from 3, then 80% probability
  tempo +2                                // Gradually accelerate
}
```

---

## 9. Session Definition (session)

Defines the playback order of scenes. Used to describe the overall structure of a song.

```
session main {
  intro [repeat 4]
  verse [repeat 8]
  chorus [repeat 8]
  verse [repeat 8]
  chorus [repeat 16]
  outro                    // Omitting count = 1 time
}
```

Sessions can also be overwritten by eval. Overwriting takes effect from the next scene transition.

Adding `[loop]` to a scene within a session causes it to loop infinitely and not advance to the next. To advance, eval a new play command.

```
session jam {
  intro [repeat 4]
  verse [loop]             // Stays here. Advance manually
  chorus [repeat 8]
  outro
}
```

---

## 10. Playback Control

### 10.1 Scene Playback

```
// Play once (default)
play verse

// Specify repeat count
play chorus [repeat 8]

// Infinite loop
play verse [loop]
```

- `play scene_name` — play once and stop
- `play scene_name [repeat N]` — repeat N times and stop
- `play scene_name [loop]` — infinite loop. Continues until the next play is eval'd

### 10.2 Session Playback

```
// Play once
play session main

// Loop the entire session infinitely (returns to the beginning after reaching the end)
play session main [loop]

// Repeat the entire session N times
play session main [repeat 3]
```

### 10.3 Stop

```
// Stop all
stop

// Mute a specific clip only
stop drums_a
```

---

## 11. Error Handling

Basic principle: **Never stop the music.** All errors are "notify but do not affect playback."

### 11.1 eval Failure

The engine's internal state is not modified at all. Playback continues with the previous state. The error is only displayed in the Neovim eval result window.

### 11.2 Undefined References

If a scene references a clip name that has not been eval'd yet, only that slot is silent. Other clips still play. If that clip is eval'd later, it starts playing from the beginning of the next loop.

```
scene verse {
  drums_a          // Defined → plays
  bass_a           // Undefined → silent (not an error)
  lead_a           // Defined → plays
}
```

### 11.3 Deletion Operations

There is no delete operation. Only overwriting. To empty something, eval an empty clip.

### 11.4 Internal Engine Panics

Rust panics are caught, maintaining the MIDI clock and current playback state. A stack trace is output to the log.

### 11.5 MIDI Output Errors

If a MIDI port disappears (e.g., USB disconnection), output to that device is skipped while other devices continue playing. When the port returns, automatic reconnection is attempted.

### 11.6 Neovim Disconnection

The engine continues playback as-is. Restarting Neovim and reconnecting allows coding to continue from where it left off.

---

## 12. Grammar Rules Summary

- Each block (device, instrument, kit, clip, scene, session, tempo, play, stop, include, var) can be independently parsed and eval'd
- Eval'ing a block with the same name overwrites it
- Overwriting a clip causes scenes using that clip to switch to the new content at the start of the next loop
- Overwriting a session takes effect from the next scene transition
- Exceeding bars results in truncation with a warning, not an error
- `>N` allows forced jumping to the beginning of a bar
- Lines with the same instrument name are concatenated (multi-line notation). The number of bars per line is unrestricted
- Comments use `//` to end of line
- Note names are all lowercase: `c c# db d d# eb e f f# gb g g# ab a a# bb b`
- Octave and duration for pitched instruments carry over from the previous value (defaults at clip start are o4, :4). Maintained across lines
- Both single notes and chord names use a unified 3-section format with `:` separators (`c:3:8`, `cm7:4:2`). `::` omits octave and changes only duration (`c::8`, `cm7::1`)
- `/` is reserved for future slash chords (chord inversions with a specified bass note)
- The chord name suffixes `Maj` and `M` have the same meaning (aliases)
- Within chords, the first note's octave becomes the reference; subsequent ones can be omitted
- Drum step sequencer notation and pitched instrument notation are not mixed within a clip (determined by whether a kit is used)
- Gate ratios (gate_normal / gate_staccato) can be set per instrument. Per-note control is available via staccato `'` and direct gate specification `gN`
- If the Gate Off period is less than 5ms, a minimum of 5ms is enforced (except for gate_normal: 100 legato)
- CC automation uses step mode (shared resolution) and time-based + interpolation mode (`@bar.beat`, `-` for linear, `-exp` for exponential curve)
- `var name = value` defines variables; referenced without `$`. Variable lookup takes priority; if not found, treated as a literal
- Scope is two-level: global (top-level) and block (inside `{}`). Inner scope takes priority
- Global variables from included files are merged into the caller. Name conflicts are resolved by last-eval-wins
- Duplicate includes of the same file are silently skipped
- Time signature is specified per clip (default is 4/4 if omitted)
- Scale is a global setting + can be overridden per clip (hint information for LSP completion, does not affect playback)
- Tempo is a global setting + can have change specifications within scenes
- All errors do not stop playback. Notification only
