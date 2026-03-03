# lcvgc (Live CV Gate Coder) DSL仕様


<!-- vim-markdown-toc GFM -->

* [概要](#概要)
* [起動オプション](#起動オプション)
* [1. デバイス定義 (device)](#1-デバイス定義-device)
    * [1.1 利用可能なMIDIポートの確認 (list_ports)](#11-利用可能なmidiポートの確認-list_ports)
* [2. 楽器定義 (instrument)](#2-楽器定義-instrument)
    * [Gate比率のデフォルト値](#gate比率のデフォルト値)
* [3. キット定義 (kit)](#3-キット定義-kit)
* [4. テンポ (tempo)](#4-テンポ-tempo)
* [4.1 スケール (scale)](#41-スケール-scale)
* [5. ファイル分割 (include)](#5-ファイル分割-include)
* [6. 変数 (var)](#6-変数-var)
    * [6.1 スコープ](#61-スコープ)
    * [6.2 evalでの再定義](#62-evalでの再定義)
    * [6.3 includeとの関係](#63-includeとの関係)
    * [6.4 予約語](#64-予約語)
* [7. クリップ定義 (clip)](#7-クリップ定義-clip)
    * [7.1 barsオプション](#71-barsオプション)
    * [7.2 拍子 (time)](#72-拍子-time)
    * [7.3 スケール指定 (scale)](#73-スケール指定-scale)
    * [7.4 音程楽器の記法](#74-音程楽器の記法)
        * [パースルール](#パースルール)
    * [7.5 省略記法](#75-省略記法)
        * [単音の省略](#単音の省略)
        * [コード名の省略](#コード名の省略)
        * [単音とコード名の混在](#単音とコード名の混在)
        * [和音内の省略](#和音内の省略)
    * [7.6 繰り返し](#76-繰り返し)
    * [7.7 アーティキュレーション（Gate制御）](#77-アーティキュレーションgate制御)
        * [通常（ノーマル）](#通常ノーマル)
        * [スタッカート `'`](#スタッカート-)
        * [Gate比率の直接指定 `gN`](#gate比率の直接指定-gn)
        * [組み合わせ](#組み合わせ)
        * [記法まとめ](#記法まとめ)
        * [Gate長の計算](#gate長の計算)
        * [リトリガーの保証](#リトリガーの保証)
        * [最小Gate Off期間](#最小gate-off期間)
    * [7.8 複数行記述](#78-複数行記述)
    * [7.9 小節ジャンプ (`>N`)](#79-小節ジャンプ-n)
    * [7.10 和音（角括弧記法）](#710-和音角括弧記法)
    * [7.11 コード名記法](#711-コード名記法)
    * [7.12 アルペジオ](#712-アルペジオ)
    * [7.13 ドラム（ステップシーケンサー記法）](#713-ドラムステップシーケンサー記法)
        * [ヒット記号](#ヒット記号)
        * [`|` ショートカット](#-ショートカット)
        * [繰り返し](#繰り返し)
        * [確率行](#確率行)
    * [7.14 CCオートメーション](#714-ccオートメーション)
        * [ステップ方式](#ステップ方式)
        * [時間指定方式](#時間指定方式)
        * [指数カーブ補間](#指数カーブ補間)
        * [両方式の混在](#両方式の混在)
* [8. シーン定義 (scene)](#8-シーン定義-scene)
    * [8.1 確率](#81-確率)
    * [8.2 シャッフル](#82-シャッフル)
    * [8.3 重み付きシャッフル](#83-重み付きシャッフル)
    * [8.4 テンポ変化](#84-テンポ変化)
    * [8.5 組み合わせ](#85-組み合わせ)
* [9. セッション定義 (session)](#9-セッション定義-session)
* [10. 再生制御](#10-再生制御)
    * [10.1 シーン再生](#101-シーン再生)
    * [10.2 セッション再生](#102-セッション再生)
    * [10.3 停止](#103-停止)
* [11. エラーハンドリング](#11-エラーハンドリング)
    * [11.1 eval失敗](#111-eval失敗)
    * [11.2 未定義の参照](#112-未定義の参照)
    * [11.3 削除操作](#113-削除操作)
    * [11.4 エンジン内部のパニック](#114-エンジン内部のパニック)
    * [11.5 MIDI出力エラー](#115-midi出力エラー)
    * [11.6 Neovim接続断](#116-neovim接続断)
* [12. 文法ルールまとめ](#12-文法ルールまとめ)

<!-- vim-markdown-toc -->

## 概要

lcvgcは、テキストベースのDSLでMIDIシーケンスを記述し、リアルタイムに評価・再生するライブコーディングエンジンである。エディタから任意のブロックを選択して評価（eval）することで、再生中の楽曲をリアルタイムに変更できる。

モジュラーシンセ（CV/Gate）からMIDIシンセ全般のシーケンスに対応する。

ファイル拡張子は `.cvg` を使用する。

---

## 起動オプション

```
lcvgc [OPTIONS]

OPTIONS:
  --file <path>          起動時に読み込むDSLファイル (.cvg)
  --port <N>             LSPサーバーのリッスンポート。デフォルト: 5555
  --midi-device <name>   MIDI出力デバイス名。省略でシステムデフォルト
  --log-level <level>    ログレベル (error, warn, info, debug)。デフォルト: info
  --config <path>        設定ファイルパス。デフォルト: ~/.config/lcvgc/config.toml
  -V, --version          バージョン表示
  -h, --help             ヘルプ表示
```

**例:**

```bash
# デフォルト設定で起動
$ lcvgc

# ファイルを読み込んで起動
$ lcvgc --file my_song.cvg

# ポートとMIDIデバイスを指定
$ lcvgc --port 7777 --midi-device "IAC Driver Bus 1"

# デバッグログ付きで起動
$ lcvgc --file live.cvg --log-level debug
```

`--file` で指定したファイルは起動時に全ブロックが自動evalされる。Neovimから `:LcvgcEval` せずに初期状態をセットアップしたい場合に便利。

---

## 1. デバイス定義 (device)

MIDIポートに名前を付ける。ポート名はOS上でMIDIデバイスとして認識されている名前を指定する。

```
device mutant_brain {
  port "Mutant Brain"
}

device volca_keys {
  port "volca keys"
}
```

### 1.1 利用可能なMIDIポートの確認 (list_ports)

`device` の `port` に指定するポート名は、OS上でMIDIデバイスとして認識されている名前と一致している必要がある。利用可能なMIDIポートの一覧は、エンジンのJSONプロトコルで `list_ports` コマンドを送信して取得できる。

**リクエスト:**

```json
{"type": "list_ports"}
```

**レスポンス:**

```json
{
  "success": true,
  "ports": [
    {"name": "Mutant Brain:Mutant Brain MIDI 1 28:0", "direction": "out"},
    {"name": "volca keys:volca keys MIDI 1 24:0", "direction": "out"},
    {"name": "Mutant Brain:Mutant Brain MIDI 1 28:0", "direction": "in"},
    {"name": "nanoKONTROL2:nanoKONTROL2 MIDI 1 32:0", "direction": "in"}
  ]
}
```

| フィールド | 型 | 説明 |
|-----------|------|------|
| `name` | string | OS上のMIDIポート名。`device` の `port` にそのまま指定できる |
| `direction` | string | `"out"` = MIDI出力ポート、`"in"` = MIDI入力ポート |

Neovimプラグインは `device` ブロック内の `port` 補完時にエンジンへ `list_ports` を送信し、取得したポート名を補完候補として提示する。エンジンとエディタが別ホスト（例: エディタはWSL2、エンジンはWindows Native）で動作する構成でも、エンジン側の実際のMIDIポートが取得できる。`device` の `port` には `direction: "out"` のポート名を指定する。

---

## 2. 楽器定義 (instrument)

device + MIDIチャンネルの組み合わせに名前を付ける。ドラム系は固定ノートを指定する。Gate比率でNote On〜Note Offの期間を制御する。CCマッピングでコントロールチェンジにエイリアスを付けられる。

```
instrument bass {
  device mutant_brain
  channel 1
  gate_normal 80           // 通常のGate比率 (%)。省略時: 80
  gate_staccato 40         // スタッカート時のGate比率 (%)。省略時: 40
  cc cutoff 74             // CC#74 に "cutoff" というエイリアスを付ける
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
  gate_normal 100          // 100 = レガート（Gate Offなし）
  gate_staccato 60
  cc pan 10
}

instrument keys {
  device mutant_brain
  channel 3
}
// gate_normal, gate_staccato 省略 → デフォルト値が適用される

// ドラム系: 固定ノートを持つ
instrument bd {
  device mutant_brain
  channel 10
  note c2
  gate_normal 50           // ドラムでもGate制御可能
  gate_staccato 20
}

// モジュラーシンセのアルゴリズム切り替え等
instrument mod_osc {
  device mutant_brain
  channel 4
  cc algorithm 12          // モジュール固有のCC番号
  cc waveform 14
}
```

### Gate比率のデフォルト値

| パラメータ | デフォルト値 |
|-----------|-------------|
| `gate_normal` | 80 |
| `gate_staccato` | 40 |

---

## 3. キット定義 (kit)

ドラム系楽器をまとめて定義する。deviceはキットレベルで指定する。各楽器に `gate_normal`, `gate_staccato` を指定可能（省略時はデフォルト値）。

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

## 4. テンポ (tempo)

グローバルに設定する。独立してeval可能。

```
// リテラル値で設定
tempo 120

// 即座に変更（evalするだけ）
tempo 140
```

scene内でテンポの変化を指定できる。

```
// ループごとに+5bpm
scene buildup {
  drums_a
  bass_a
  tempo +5
}

// リテラル値で固定に戻す
scene drop {
  drums_a
  bass_a
  tempo 120
}
```

---

## 4.1 スケール (scale)

グローバルに設定する。独立してeval可能。clipの `[scale ...]` で上書きできる。スケール指定は再生動作には影響せず、LSP補完のためのヒント情報。

```
// グローバルに設定
scale c minor

// 即座に変更（evalするだけ）
scale d dorian
```

clipレベルで上書きする場合:

```
scale c minor

clip bass_a [bars 1] {
  // グローバルのscale (c minor) が適用される
  bass c:3:8 d eb f::4 g::2
}

clip lead_a [bars 1] [scale d dorian] {
  // clipレベルで上書き → d dorian が適用される
  lead d:3:4 e f g
}
```

clipに `[scale ...]` が指定されていない場合、グローバルのscaleが適用される。グローバルのscaleも未設定の場合、LSPは音名とコード名の一般的な補完のみ行う。

---

## 5. ファイル分割 (include)

相対パスで別の `.cvg` ファイルを読み込む。循環includeはパースエラーになる。同じファイルを2回以上includeした場合、2回目以降はサイレントにスキップされる（エンジンが読み込み済みパスを管理する）。

```
include "./setup.cvg"
include "./clips/drums.cvg"
include "./clips/bass.cvg"
```

```
// setup.cvg
var dev = mutant_brain

// drums.cvg
include "./setup.cvg"       // 1回目: 読み込む

// song.cvg
include "./setup.cvg"       // 読み込む
include "./drums.cvg"       // drums.cvg内のinclude "setup.cvg" はスキップ
```

LSPではファイルパスの補完が効く。

---

## 6. 変数 (var)

`var 名前 = 値` で変数を定義する。参照は `$` なしで名前をそのまま書く。パーサーは値の位置にある識別子をまずスコープ内の変数として探し、見つかれば展開、見つからなければリテラルとして扱う。

```
// グローバル変数
var dev = mutant_brain
var default_gate = 80
var bass_ch = 1
var cutoff_cc = 74

instrument bass {
  device dev                    // 変数dev → mutant_brain
  channel bass_ch               // 変数bass_ch → 1
  gate_normal default_gate      // 変数default_gate → 80
  cc cutoff cutoff_cc           // 変数cutoff_cc → 74
}

instrument lead {
  device mutant_brain           // 変数を通さず直接書いてもok
  channel 2
}
```

### 6.1 スコープ

グローバル（トップレベル）とブロック（`{}` 内）の2段スコープ。内側が優先（シャドーイング）。

```
var ch = 1

instrument bass {
  var ch = 3                    // ブロック内で別の値
  channel ch                    // → 3
}

instrument lead {
  channel ch                    // → 1（グローバル）
}
```

### 6.2 evalでの再定義

グローバル変数を再evalすれば値が変わる。ただし、既にevalされたブロックには影響しない（次にそのブロックをevalした時に反映される）。

```
var dev = mutant_brain
// bassをeval → mutant_brainを使用

var dev = keystep
// bassを再eval → keystepを使用
```

### 6.3 includeとの関係

includeしたファイルのグローバル変数は呼び出し元にマージされる。名前が衝突した場合は後からevalされた方が勝つ。

```
// config.cvg
var dev = mutant_brain
var default_gate = 80

// song.cvg
include "./config.cvg"          // dev, default_gate が使える
var default_gate = 90           // 上書き

instrument bass {
  device dev                    // → mutant_brain
  gate_normal default_gate      // → 90（上書き後の値）
}
```

### 6.4 予約語

以下のキーワードは変数名に使えない:

`device`, `instrument`, `kit`, `clip`, `scene`, `session`, `include`, `tempo`, `play`, `stop`, `var`, `port`, `channel`, `note`, `gate_normal`, `gate_staccato`, `cc`, `use`, `resolution`, `arp`, `bars`, `time`, `scale`, `repeat`, `loop`

---

## 7. クリップ定義 (clip)

再生パターンの単位。evalで同名のclipを再送信すると上書きされ、再生中のsceneが使用しているclipは次のループ頭から新しい内容に切り替わる。

### 7.1 barsオプション

```
// bars指定: N小節に合わせる
// 足りなければ末尾を休符で埋める
// あふれたらN小節分の長さで切り捨て（ワーニング表示、エラーにはならない）
clip bass_a [bars 1] {
  bass c:3:8 c:3:8 eb:3:8 f:3:4 g:3:2
}

// bars省略: clip内の音符の合計長でループする
// 異なる長さのclipを同時に鳴らすとポリリズムになる
clip bass_poly {
  bass c:3:4 eb:3:4 f:3:4
}
```

### 7.2 拍子 (time)

clipごとに拍子を指定できる。省略は4/4。

```
// 3/4拍子
clip waltz_bass [bars 2] [time 3/4] {
  bass c:3:4 e g
  bass f:3:4 a c
}

// 4/4（デフォルト、省略可）
clip drums_a [bars 1] {
  use tr808
  resolution 16
  bd x|x|x|x          // 16ステップ = 4拍
}

// 3/4のドラム
clip drums_waltz [bars 1] [time 3/4] {
  use tr808
  resolution 16
  bd x|x|x             // 12ステップ = 3拍
}
```

### 7.3 スケール指定 (scale)

clipにスケールを指定すると、LSPがそのスケールのダイアトニックコードと進行候補を補完に出す。スケール指定は再生動作には影響せず、LSP補完のためのヒント情報。

```
// スケール指定
clip chords_a [bars 4] [scale c minor] {
  keys cm7:4:2       // LSPが次のコード候補を出す:
                     //   fm7 (IVm7), gm7 (Vm7), g7 (V7),
                     //   ebM7 (bIII), abM7 (bVI), bb7 (bVII), dm7b5 (IIm7b5)
  keys fm7:3:2
  keys g7:3:2
  keys cm7:4:1
}

// メジャースケール
clip chords_b [bars 4] [scale g major] {
  keys gM7:4:2       // I → 候補: am7(II), bm7(III), cM7(IV), d7(V), em7(VI)
  keys cM7:4:2
  keys d7:3:2
  keys gM7:4:1
}

// モードも指定可能
clip chords_c [bars 4] [scale d dorian] {
  keys dm7:4:2
  keys g7:3:2
  keys em7:3:2
  keys dm7:4:1
}
```

LSP補完の段階的な動作:

- `[scale ` の後 → ルート音の候補: `c`, `c#`, `db`, `d`, ... `b`
- `[scale c ` の後 → スケールタイプの候補: `major`, `minor`, `harmonic_minor`, `melodic_minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`
- scale指定済みclip内でコードを書く位置 → そのスケールのダイアトニックコード全て
- 直前のコードがある場合 → 進行テーブルに基づく次のコード候補（degree情報付き）

サポートするスケールタイプ:

| スケール | ダイアトニックコード例 (key=c) |
|----------|------|
| major | cM7, dm7, em7, fM7, g7, am7, bm7b5 |
| minor (natural) | cm7, dm7b5, ebM7, fm7, gm7, abM7, bb7 |
| harmonic_minor | cmM7, dm7b5, ebM7#5, fm7, g7, abM7, bdim7 |
| melodic_minor | cmM7, dm7, ebM7#5, f7, g7, am7b5, bm7b5 |
| dorian | cm7, dm7, ebM7, f7, gm7, am7b5, bbM7 |
| mixolydian | c7, dm7, em7b5, fM7, gm7, am7, bbM7 |

clipの `[scale ...]` は省略可能。省略した場合、グローバルの `scale` 設定（セクション4.1参照）が適用される。グローバルも未設定の場合、LSPは音名とコード名の一般的な補完のみ行う。

### 7.4 音程楽器の記法

書式: `楽器名 音名orコード名[:オクターブ][:音長] ...`

単音・コード名ともに `:` 区切りの3セクション形式で統一する。

```
clip bass_a [bars 1] {
  // フル表記（3セクション）
  bass c:3:8 c:3:8 eb:3:8 f:3:4 g:3:2

  // 省略表記: オクターブと音長は直前の値を引き継ぐ
  // clip先頭のデフォルトは o4 :4
  bass c:3:8 c eb f::4 g::2
  //   c:3:8 → o3, :8がセットされる
  //   c     → o3, :8を引き継ぐ
  //   eb    → o3, :8を引き継ぐ
  //   f::4  → o3を引き継ぎ、音長だけ:4に変更（::でオクターブ省略）
  //   g::2  → o3を引き継ぎ、音長だけ:2に変更
}

clip lead_a [bars 1] {
  lead eb:5:4 d::8 c bb:4:2
  //   eb:5:4 → o5, :4
  //   d::8   → o5引き継ぎ, :8に変更
  //   c      → o5, :8
  //   bb:4:2 → o4, :2
}
```

- 音名: `c c# db d d# eb e f f# gb g g# ab a a# bb b` (全て小文字)
- コード名: `cm7`, `fM7`, `g7` 等（サフィックス一覧はセクション7.11参照）
- オクターブ: `0-9` — `:` 区切りで指定。省略で直前を引き継ぐ
- 音長: `1`=全音符, `2`=半音符, `4`=四分, `8`=八分, `16`=十六分, 付点は`4.` `8.`のように`.`を付ける。省略で直前を引き継ぐ
- 休符: `r[:音長]` (音長は省略で直前を引き継ぐ)
- スタッカート: `'` をノートの末尾に付ける → `gate_staccato` が適用される
- Gate直接指定: `gN` をノートの末尾に付ける → Gate比率N%で再生

#### パースルール

単音・コード名ともに共通のパースルール。

| 記法 | オクターブ | 音長 | 説明 |
|------|-----------|------|------|
| `c` / `cm7` | 引き継ぎ | 引き継ぎ | 両方省略 |
| `c:3` / `cm7:4` | 3 / 4 | 引き継ぎ | オクターブのみ指定 |
| `c:3:8` / `cm7:4:2` | 3 / 4 | 8分 / 2分 | フル表記 |
| `c::8` / `cm7::2` | 引き継ぎ | 8分 / 2分 | オクターブ省略、音長だけ変更 |

### 7.5 省略記法

オクターブと音長は直前の値を引き継ぐ。clip先頭のデフォルトは o4, :4。この引き継ぎは行をまたいでも維持される。

#### 単音の省略

```
clip bass_a [bars 2] {
  bass c:3:8 c eb f::4 g::2
  //   c:3:8 → o3, :8
  //   c     → o3, :8（両方引き継ぎ）
  //   eb    → o3, :8
  //   f::4  → o3引き継ぎ, :4に変更（::でオクターブ省略）
  //   g::2  → o3引き継ぎ, :2に変更

  // 2行目も前行末の状態(o3, :2)を引き継ぐ
  bass ab::8 g f eb::4 c::2
}
```

#### コード名の省略

コード名でもパースルールは同じ。`::` でオクターブ省略+音長変更も同じ。

```
clip chords_a [bars 4] {
  keys cm7:4:2       // o4, :2
  keys fm7::1        // o4引き継ぎ, :1に変更（::でオクターブ省略）
  keys g7            // o4, :1 両方引き継ぎ
  keys cm7:3:4       // o3に変更, :4に変更
}
```

#### 単音とコード名の混在

同じclip内で単音とコード名を書いた場合も、オクターブ・音長は一貫して引き継がれる。

```
clip mixed_a [bars 2] {
  keys cm7:4:2                   // o4, :2
  keys [f:3 a c eb]:2            // o3（明示）, :2
  keys bbM7::1                   // o3引き継ぎ, :1（::でオクターブ省略）
}
```

#### 和音内の省略

和音（角括弧記法）内では、最初の音のオクターブが基準になり以降省略可。

```
keys [c:4 eb g bb]:2         // c:4でo4確定、eb,g,bbはo4
keys [f:3 a c eb]:2          // f:3でo3、a,c,ebはo3
keys [bb:3 d:4 f a]:1        // オクターブまたぎは明示
```

### 7.6 繰り返し

`()*N` でフレーズを繰り返す。ドラムのステップシーケンサー記法と共通の記法。

```
clip bass_a [bars 4] {
  // フレーズ全体を4回繰り返す
  bass (c:3:8 c eb f::4 g::2)*4

  // 一部だけ繰り返し
  bass c:3:8 (c eb)*3 f::4 g::2
}

clip chords_a [bars 4] {
  // コード進行の繰り返し
  keys (cm7:4:2 fm7::1)*2
}
```

繰り返し内のオクターブ・音長の引き継ぎは、繰り返しの各回で先頭に戻らず、前回末尾の状態を引き継ぐ。

### 7.7 アーティキュレーション（Gate制御）

音符のGate長（Note On〜Note Offの期間）をアーティキュレーションで制御する。

#### 通常（ノーマル）

修飾なしのノートには、instrumentに設定された `gate_normal` が適用される。

```
clip bass_a [bars 1] {
  bass c:3:8 c eb f::4 g::2
  // → 各ノートの音長 × 80%（bassのgate_normal）がGate On期間
}
```

#### スタッカート `'`

ノートの末尾に `'` を付けると `gate_staccato` が適用される。

```
clip bass_stac [bars 1] {
  bass c:3:8' c' eb' f::4' g::2
  // → 各ノートの音長 × 40%（bassのgate_staccato）がGate On期間
}
```

#### Gate比率の直接指定 `gN`

特定のノートだけGate比率を変更したい場合、`gN` で直接パーセンテージを指定する。

```
clip bass_mix [bars 1] {
  bass c:3:8 d eg95 f::4 g::2
  // → eのみ95%、他はgate_normal(80%)
}
```

#### 組み合わせ

付点音符 + スタッカート、付点音符 + Gate直接指定も可能。

```
clip bass_combo [bars 1] {
  bass c:3:4.' d:8              // 付点四分 + スタッカート
  bass e:3:4.g30 f:8            // 付点四分 + Gate 30%
}
```

#### 記法まとめ

| 記法 | 意味 | 例 |
|------|------|------|
| `c:3:4` | 通常（gate_normal適用） | Gate On = 音長×80% |
| `c:3:4'` | スタッカート（gate_staccato適用） | Gate On = 音長×40% |
| `c:3:4.` | 付点音符（音長1.5倍、gate_normal適用） | Gate On = 付点音長×80% |
| `c:3:4.'` | 付点 + スタッカート | Gate On = 付点音長×40% |
| `c:3:4g95` | Gate比率を直接指定（95%） | Gate On = 音長×95% |
| `c:3:4.g30` | 付点 + Gate比率直接指定（30%） | Gate On = 付点音長×30% |

#### Gate長の計算

```
gate_duration = note_duration × (gate_percent / 100)
rest_duration = note_duration - gate_duration
```

例: BPM120、四分音符（500ms）、gate_normal: 80 の場合、Gate On: 400ms、Gate Off: 100ms。

#### リトリガーの保証

Gate Off期間を設けることで、連続するノートに対してEG（エンベロープジェネレーター）が毎回Attackフェーズからリトリガーされることを保証する。

`gate_normal: 100`（レガート）の場合はGate Off期間がなくなるため、シンセ側のリトリガー設定に依存する。

#### 最小Gate Off期間

Gate比率の計算結果としてGate Off期間が5ms未満になる場合、最小5msのGate Off期間を確保する（リトリガー保証のため）。ただし `gate_normal: 100` の場合はこの制限を適用しない（意図的なレガート）。

### 7.8 複数行記述

clip内で同じ楽器名の行が続くと、前の行の続きとして連結される。長いclipを読みやすく分割できる。オクターブ・音長の引き継ぎは行をまたいで維持される。1行に何小節分書くかは自由。

```
// 4小節を1小節ずつ4行で
clip bass_a [bars 4] {
  bass c:3:8 c eb f::4 g::2
  bass ab:3:8 g f eb::4 c::2
  bass c:3:4 eb f g
  bass ab:3:2 g::2
}

// 4小節を2小節ずつ2行で
clip bass_b [bars 4] {
  bass c:3:8 c eb f::4 g::2 ab:3:8 g f eb::4 c::2
  bass c:3:4 eb f g ab:3:2 g::2
}

// 1行で全部書いてもいい
clip bass_c [bars 4] {
  bass c:3:8 c eb f::4 g::2 ab:3:8 g f eb::4 c::2 c:3:4 eb f g ab:3:2 g::2
}
```

ドラムも同様。同じ楽器名の行が連結される。確率行は直前のヒット行にだけ対応する。

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

### 7.9 小節ジャンプ (`>N`)

`>N` で強制的にN小節目（1始まり）の頭に現在位置を移動する。ライブコーディング中に小節の計算が合わなくなった時に便利。

```
clip bass_a [bars 4] {
  // >N で指定小節の頭に強制移動
  bass c:3:1 d:3:1 >3 e:3:4 f:3:4 g:3:4 a:3:4 >4 g:3:1
  //   ^^^^^^^^^^^^     1-2小節目
  //              >3  3小節目の頭にジャンプ
  //                 ^^^^^^^^^^^^^^^^^^^^^^^^ 3小節目
  //                                      >4 4小節目の頭にジャンプ
  //                                         ^^^^ 4小節目
}
```

ルール:

- `>N` はN小節目の頭（1始まり）に現在位置を強制移動する
- 現在位置がN小節目より手前 → 休符で埋める
- 現在位置がN小節目より先 → 超過分を切り捨て
- barsの範囲外の `>N` はパースエラー（例: `[bars 4]` で `>5`）

ドラムのステップシーケンサー記法でも使用できる。`|`（拍頭ショートカット）とは別の記号なので混乱しない。

```
clip drums_a [bars 4] {
  use tr808
  resolution 16

  bd    x|x|x|x >2 x.x.|x|x.x.|x >3 x|x|x|x >4 x.x.x.x.x.x.x.x.
  snare |x||x   >2 |x||X         >3 |x||x   >4 |x|x.x.X...
}
```

### 7.10 和音（角括弧記法）

角括弧で括ることで同時発音する。同一MIDIチャンネルに複数のノートオンを送る。

```
clip chords_a [bars 2] {
  keys [c:4 eb g bb]:2         // 最初のc:4でo4確定、以降省略可
  keys [f:3 a c eb]:2          // f:3でo3に
  keys [bb:3 d:4 f a]:1        // オクターブまたぎは明示
}

// 2音だけも可
clip fifths [bars 1] {
  keys [c:3 g:3]:2
  keys [f:3 c:4]:2
}
```

### 7.11 コード名記法

書式: `楽器名 コード名:オクターブ:音長`

```
clip chords_named [bars 2] {
  keys cm7:4:2
  keys f7:3:2
  keys bbM7:3:1              // M7 = Maj7 のエイリアス
}

// Maj と M は両方使える
clip chords_alias [bars 2] {
  keys cMaj7:4:2             // Maj7
  keys cM7:4:2               // M7（同じ意味）
}
```

コード名サフィックス:

| サフィックス | エイリアス | 意味 |
|-------------|-----------|------|
| `M` | `Maj` | メジャー |
| `M7` | `Maj7` | メジャーセブンス |
| `m` | — | マイナー |
| `m7` | — | マイナーセブンス |
| `7` | — | ドミナントセブンス |
| `dim` | — | ディミニッシュ |
| `dim7` | — | ディミニッシュセブンス |
| `aug` | — | オーギュメント |
| `m7b5` | — | ハーフディミニッシュ |
| `mM7` | `mMaj7` | マイナーメジャーセブンス |
| `sus4` | — | サスフォー |
| `sus2` | — | サスツー |
| `6` | — | シックス |
| `m6` | — | マイナーシックス |
| `9` | — | ナインス |
| `m9` | — | マイナーナインス |
| `add9` | — | アドナイン |
| `13` | — | サーティーンス |
| `m13` | — | マイナーサーティーンス |

`/` は将来のオンコード（分数コード）用に予約する。

角括弧記法とコード名記法は混在可能。

```
clip chords_mixed [bars 2] {
  keys cm7:4:2
  keys [f:3 a:3 c:4 eb:4]:2    // ボイシングにこだわりたい箇所だけ個別指定
  keys bbM7:3:1
}
```

### 7.12 アルペジオ

和音の後に `arp(方向, 音符解像度)` を付ける。

```
clip arp_a [bars 1] {
  keys [c:4 eb:4 g:4 bb:4]:1 arp(up, 16)      // 上昇、16分音符間隔
}

clip arp_b [bars 1] {
  keys [c:4 eb:4 g:4 bb:4]:1 arp(down, 16)    // 下降
}

clip arp_c [bars 1] {
  keys [c:4 eb:4 g:4 bb:4]:1 arp(random, 8)   // ランダム、8分音符間隔
}

clip arp_d [bars 2] {
  keys cm7:4:1 arp(updown, 16)             // 上って下る
}
```

- 方向: `up`, `down`, `updown`, `random`
- 音符解像度: `4`, `8`, `16` 等（各音の発音間隔）

### 7.13 ドラム（ステップシーケンサー記法）

`use` でkitを指定し、`resolution` で1文字あたりの音符解像度を設定する。

```
clip drums_a [bars 1] {
  use tr808
  resolution 16          // 1文字 = 16分音符

  bd    x|x|x|x
  snare |x||x
  hh    x.o.x.o.x.o.x.o.
}
```

#### ヒット記号

| 記号 | 意味 | MIDIベロシティ |
|------|------|----------------|
| `x` | 通常ヒット | 100 |
| `X` | アクセント | 127 |
| `o` | ゴーストノート | 40 |
| `.` | 休符 | - |

#### `|` ショートカット

`|` は現在位置から次の拍頭（resolution 16なら4文字境界）まで休符 `.` で埋める。

```
bd    x|x|x|x
// 展開: x...x...x...x...

snare |x||x
// 展開: ....x.......x...
```

- 先頭の `|` は最初の拍を丸ごと休符にする
- 連続する `||` は拍を丸ごとスキップ

#### 繰り返し

`()*N` でステップパターンを繰り返す。音程楽器の繰り返し記法（セクション7.6）と共通。

```
hh    (x.x.)*4              // x.x. を4回繰り返す
hh    (x.o.)*3 xxxx         // 最後の拍だけ変える
```

#### 確率行

ヒット行の直下に、ステップごとの発音確率を記述できる（任意）。

```
clip drums_a [bars 1] {
  use tr808
  resolution 16

  hh    x.o.x.o.x.o.x.o.
  // 確率: ゴーストを間引いてランダム感を出す
        ..5...7...3...5.
}
```

- 数字 `1`-`9` = 10%-90%
- `.` またはスペース = 100%（省略可）
- `0` = 0%（実質ミュート）
- ヒットがない位置の数字は無視される
- 確率行を省略すれば全て100%
- 判定はループのたびに毎回行う

### 7.14 CCオートメーション

instrumentに定義したCCエイリアスを使って、clip内でMIDI Control Changeメッセージを送信する。ステップ方式と時間指定+補間方式の2種類がある。

#### ステップ方式

ドラムの `resolution` を共有する。値は0-127（10進数）。

```
clip bass_a [bars 1] {
  resolution 16
  bass c:3:8 c eb f::4 g::2

  // 16ステップで値を指定
  bass.cutoff    0 10 20 30 40 50 60 70 80 90 100 110 120 127 127 127
  bass.resonance 40 40 40 40 60 60 60 60 80 80 80 80 127 127 127 127
}
```

resolutionを指定せず音程楽器だけのclipでは、ステップ方式は使えない（時間指定方式を使う）。

#### 時間指定方式

`値@小節.拍` で任意のタイミングにCC値を送信する。

```
clip bass_b [bars 4] {
  bass c:3:8 c eb f::4 g::2

  // ポイント指定（即座に値が変わる）
  bass.cutoff 0@1.1 64@2.1 127@3.1 64@4.1

  // 線形補間: - で繋ぐ（間の値を自動生成）
  bass.cutoff 0@1.1-127@3.1 64@4.1

  // アルゴリズム切り替え: 2小節目の頭でバンと変える
  mod_osc.algorithm 0@1.1 64@2.1 127@3.1
}
```

`-` で繋ぐと、2点間を線形補間してCCメッセージを段階的に送信する。補間の送信間隔はエンジンが自動で決定する。

#### 指数カーブ補間

`-` の代わりに `-exp` を使うと指数カーブで補間する。フィルターのカットオフなど、対数的に変化するパラメータに向いている。

```
clip bass_c [bars 4] {
  // 線形補間
  bass.cutoff 0@1.1-127@4.4

  // 指数カーブ（ゆっくり上がって最後に急上昇）
  bass.cutoff 0@1.1-exp127@4.4
}
```

#### 両方式の混在

同じclip内でステップ方式と時間指定方式を別々のCCパラメータに使える。同じCCパラメータに両方式を混在させることはできない。

```
clip bass_mix [bars 2] {
  resolution 16
  bass c:3:8 c eb f::4 g::2

  // cutoffはステップ方式
  bass.cutoff 0 10 20 30 40 50 60 70 80 90 100 110 120 127 127 127

  // パンは時間指定方式
  pad.pan 0@1.1-127@2.4
}
```

---

## 8. シーン定義 (scene)

同時に鳴らすclipの組み合わせを定義する。

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

### 8.1 確率

clip名の後に数字（1-9）を付けると発音確率を指定できる。ループごとに判定する。

```
scene verse {
  drums_a
  bass_a
  lead_a 7                   // 70%の確率で鳴る
  chords_a 5                 // 50%
}
```

- `1`-`9` = 10%-90%
- 省略 = 100%

### 8.2 シャッフル

`|` で複数のclip候補を並べると、ループごとにランダムに1つ選ばれる。

```
scene chorus {
  drums_a | drums_funk       // ループごとにどちらかが鳴る
  bass_a
  lead_a
  chords_a | chords_open     // コードも毎回変わる
}
```

### 8.3 重み付きシャッフル

`*N` で重みを指定する。

```
scene verse_v2 {
  drums_a*3 | drums_funk     // drums_a 75%, drums_funk 25%
  bass_a
}
```

### 8.4 テンポ変化

scene内でテンポの変化を指定できる。

```
// ループごとに+5bpm
scene buildup {
  drums_a
  bass_a
  tempo +5
}

// リテラル値で固定に戻す
scene drop {
  drums_a
  bass_a
  tempo 120
}
```

### 8.5 組み合わせ

確率、シャッフル、テンポ変化は組み合わせ可能。

```
scene breakdown {
  drums_a | drums_poly
  bass_a 6                                // 60%で鳴る
  arp_a | arp_b | arp_c 8                 // 3つからランダム選択、さらに80%の確率
  tempo +2                                // じわじわ加速
}
```

---

## 9. セッション定義 (session)

シーンの再生順序をまとめて定義する。曲全体の構成を記述できる。

```
session main {
  intro [repeat 4]
  verse [repeat 8]
  chorus [repeat 8]
  verse [repeat 8]
  chorus [repeat 16]
  outro                    // 回数省略 = 1回
}
```

sessionもevalで上書きできる。上書きすると次のシーン切り替わり時から新しい構成に変わる。

session内のシーンに `[loop]` を付けると、そのシーンで無限ループし次に進まない。次に進ませるには新しいplayをevalする。

```
session jam {
  intro [repeat 4]
  verse [loop]             // ここで止まる。手動で次に進む
  chorus [repeat 8]
  outro
}
```

---

## 10. 再生制御

### 10.1 シーン再生

```
// 1回再生 (デフォルト)
play verse

// リピート指定
play chorus [repeat 8]

// 無限ループ
play verse [loop]
```

- `play シーン名` — 1回再生して停止
- `play シーン名 [repeat N]` — N回繰り返して停止
- `play シーン名 [loop]` — 無限ループ。次のplayをevalするまで続く

### 10.2 セッション再生

```
// 1回再生
play session main

// セッション全体を無限ループ（最後まで行ったら先頭に戻る）
play session main [loop]

// セッション全体をN回繰り返す
play session main [repeat 3]
```

### 10.3 停止

```
// 全停止
stop

// 特定clipだけミュート
stop drums_a
```

---

## 11. エラーハンドリング

基本方針: **音は絶対に止めない**。全てのエラーは「通知はするが、再生には影響しない」。

### 11.1 eval失敗

エンジンの内部状態は一切変更しない。直前の状態で再生を継続する。エラーはNeovimのeval結果ウィンドウに表示するだけ。

### 11.2 未定義の参照

sceneでまだevalされていないclip名を参照している場合、そのスロットだけ無音にする。他のclipは鳴る。後からそのclipをevalすれば次のループ頭から鳴り始める。

```
scene verse {
  drums_a          // 定義済み → 鳴る
  bass_a           // 未定義 → 無音（エラーにしない）
  lead_a           // 定義済み → 鳴る
}
```

### 11.3 削除操作

削除という操作は用意しない。上書きのみ。空にしたければ空のclipでevalする。

### 11.4 エンジン内部のパニック

Rustのパニックはcatchし、MIDIクロックと現在の再生状態を維持する。ログにスタックトレースを出力する。

### 11.5 MIDI出力エラー

MIDIポートが消えた場合（USB抜け等）、そのdeviceへの出力だけスキップし他のdeviceは鳴らし続ける。ポートが戻ったら自動で再接続を試みる。

### 11.6 Neovim接続断

エンジンはそのまま再生を続ける。Neovimを再起動して再接続すれば続きからコーディングできる。

---

## 12. 文法ルールまとめ

- 各ブロック（device, instrument, kit, clip, scene, session, tempo, play, stop, include, var）は独立してパース・eval可能
- 同名のブロックをevalすると上書きされる
- clipを上書きすると、そのclipを使用中のsceneは次のループ頭から新しい内容に切り替わる
- sessionを上書きすると、次のシーン切り替わり時から新しい構成に変わる
- barsを超過した場合はエラーではなく切り捨て（ワーニング表示）
- `>N` で小節頭への強制ジャンプが可能
- 同じ楽器名の行は連結される（複数行記述）。1行あたりの小節数は自由
- コメントは `//` で行末まで、または `/* ... */` で複数行一括（ネスト対応）
- 音名は全て小文字: `c c# db d d# eb e f f# gb g g# ab a a# bb b`
- 音程楽器のオクターブと音長は直前の値を引き継ぐ（clip先頭のデフォルトは o4, :4）。行をまたいでも維持される
- 単音・コード名ともに `:` 区切りの3セクション形式で統一（`c:3:8`, `cm7:4:2`）。`::` でオクターブ省略+音長変更（`c::8`, `cm7::1`）
- `/` は将来のオンコード（分数コード）用に予約
- コード名サフィックスの `Maj` と `M` は同じ意味（エイリアス）
- 和音内では最初の音のオクターブが基準になり、以降省略可
- ドラムのステップシーケンサー記法と音程楽器の記法はclip内で混在しない（kitを使うかどうかで決まる）
- instrumentごとにGate比率（gate_normal / gate_staccato）を設定可能。スタッカート `'` やGate直接指定 `gN` で音符単位の制御もできる
- Gate Off期間が5ms未満の場合は最小5msを確保（gate_normal: 100のレガート時は除く）
- CCオートメーションはステップ方式（resolution共有）と時間指定+補間方式（`@小節.拍`、`-` で線形、`-exp` で指数カーブ）
- `var 名前 = 値` で変数定義、`$` なしで参照。変数優先、見つからなければリテラル
- スコープはグローバル（トップレベル）とブロック（`{}` 内）の2段。内側優先
- includeしたファイルのグローバル変数は呼び出し元にマージ。名前衝突は後勝ち
- 同じファイルの2重includeはサイレントにスキップ
- 拍子はclipごとに指定（省略は4/4）
- スケールはグローバル設定 + clipごとに上書き可能（LSP補完のヒント情報、再生動作には影響しない）
- テンポはグローバル設定 + scene内で変化指定可能
- 全てのエラーは再生を止めない。通知のみ
