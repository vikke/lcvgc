use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lcvgc::engine::clock::Clock;
use lcvgc::engine::compiler::compile_clip;
use lcvgc::engine::evaluator::Evaluator;
use lcvgc::parser::parse_source;

const SIMPLE_SOURCE: &str = r#"
tempo 140

device mb {
  port "Mutant Brain"
}

instrument bass {
  device mb
  channel 1
}
"#;

const COMPLEX_SOURCE: &str = r#"
tempo 130

device mb {
  port "Mutant Brain"
}

device volca {
  port "volca keys"
}

instrument bass {
  device mb
  channel 1
  gate_normal 80
  gate_staccato 40
}

instrument keys {
  device volca
  channel 2
}

kit tr808 {
  device mb
  bd    { channel 10, note c2 }
  snare { channel 10, note d2 }
  hh    { channel 10, note f#2 }
}

clip bass_line [bars 2] {
  bass c:3:8 c eb f::4 g::2
  bass ab:3:8 g f eb::4 c::2
}

clip keys_pad [bars 1] {
  keys cm7:4:2
}

clip drums_a [bars 1] {
  use tr808
  resolution 16
  bd    x...x...x...x...
  snare ....x.......x...
  hh    x.x.x.x.x.x.x.x
}

scene verse {
  bass_line
  keys_pad
  drums_a
}

scene bridge {
  tempo +5
  keys_pad
}

session live {
  verse [repeat 4]
  bridge
  verse [loop]
}

scale c minor

var style = dark
"#;

fn bench_parse_simple(c: &mut Criterion) {
    c.bench_function("parse_simple", |b| {
        b.iter(|| parse_source(black_box(SIMPLE_SOURCE)))
    });
}

fn bench_parse_complex(c: &mut Criterion) {
    c.bench_function("parse_complex", |b| {
        b.iter(|| parse_source(black_box(COMPLEX_SOURCE)))
    });
}

fn bench_eval_simple(c: &mut Criterion) {
    c.bench_function("eval_simple", |b| {
        b.iter(|| {
            let mut ev = Evaluator::new(120.0);
            ev.eval_source(black_box(SIMPLE_SOURCE)).unwrap()
        })
    });
}

fn bench_eval_complex(c: &mut Criterion) {
    c.bench_function("eval_complex", |b| {
        b.iter(|| {
            let mut ev = Evaluator::new(120.0);
            ev.eval_source(black_box(COMPLEX_SOURCE)).unwrap()
        })
    });
}

fn bench_compile_clip(c: &mut Criterion) {
    let mut ev = Evaluator::new(120.0);
    ev.eval_source(COMPLEX_SOURCE).unwrap();
    let clip = ev.registry().get_clip("bass_line").unwrap().clone();
    let clock = Clock::new(130.0);

    c.bench_function("compile_clip", |b| {
        b.iter(|| compile_clip(black_box(&clip), &clock, ev.registry()))
    });
}

criterion_group!(
    benches,
    bench_parse_simple,
    bench_parse_complex,
    bench_eval_simple,
    bench_eval_complex,
    bench_compile_clip,
);
criterion_main!(benches);
