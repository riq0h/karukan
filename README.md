<div align="center">
  <img src="icon.png" width="128" alt="karukan" />
  <h1>Karukan</h1>
  <p>Linux向け日本語入力システム — ニューラルかな漢字変換エンジン + fcitx5</p>

  [![CI (engine)](https://github.com/togatoga/karukan/actions/workflows/karukan-engine-ci.yml/badge.svg)](https://github.com/togatoga/karukan/actions/workflows/karukan-engine-ci.yml)
  [![CI (im)](https://github.com/togatoga/karukan/actions/workflows/karukan-im-ci.yml/badge.svg)](https://github.com/togatoga/karukan/actions/workflows/karukan-im-ci.yml)
  [![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
</div>

<div align="center">
  <img src="images/demo.gif" width="800" alt="karukan demo" />
</div>

## プロジェクト構成

| クレート | 説明 |
|---------|------|
| [karukan-im](karukan-im/) | karukan-engineを利用したfcitx5向け日本語入力システム |
| [karukan-engine](karukan-engine/) | コアライブラリ — ローマ字→ひらがな変換 + llama.cppによるニューラルかな漢字変換 |
| [karukan-cli](karukan-cli/) | CLIツール・サーバー — 辞書ビルド、Sudachi辞書生成、辞書ビューア、AJIMEE-Bench、HTTPサーバー |

## 特徴

- **ニューラルかな漢字変換**: GPT-2ベースのモデルをllama.cppで推論し、高度な日本語変換
- **コンテキスト対応**: 周辺テキストを考慮した日本語変換
- **変換学習**: ユーザーが選択した変換結果を記憶し、次回以降の変換で優先表示。予測変換（前方一致）にも対応し、入力途中でも学習済みの候補を提示
- **システム辞書**: [SudachiDict](https://github.com/WorksApplications/SudachiDict)の辞書データからシステム辞書を構築

> **Note:** 初回起動時にHugging Faceからモデルをダウンロードするため、初回の変換開始までに時間がかかります。2回目以降はダウンロード済みのモデルが使用されます。

## インストール

インストール方法は [karukan-im の README](karukan-im/README.md#install) を参照してください。

## ライセンス

MIT OR Apache-2.0 のデュアルライセンスで提供しています。

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

---

## フォーク変更点

本フォークでは、私的な使い勝手を向上させるため以下の変更を加えています。

### 機能追加・修正

#### 1. 末尾「n」の自動変換
Space変換時に末尾の `n` を自動的に「ん」に変換します。`nn` と入力する必要がなくなりました。

- 対象: `karukan-engine/src/romaji/converter.rs`

#### 2. Shift英字入力後のモード自動復帰
Shift+英字でアルファベットモードに入り、以降の入力はShiftなしでもアルファベットとして扱われます（例: Shift+L → i → n → u → x で「Linux」）。確定（Enter）やキャンセル（Escape）でEmpty状態に戻ると自動的にひらがなモードに復帰します。

- 対象: `karukan-im/src/core/engine/input.rs`, `mod.rs`

#### 3. Shift+矢印キーによる選択ベース部分変換
ComposingおよびConversion状態でShift+矢印キーを使って選択範囲を作り、Spaceを押すと選択部分のみを変換します。Conversion状態でShift+矢印を押すと変換をキャンセルしてComposingに戻り、選択が開始されます。確定（Enter）すると変換結果がComposingバッファに焼き込まれ（bake）、残りの部分を続けて変換できます。選択方向は自由で、先頭からでも末尾からでも部分変換が可能です。すべての部分変換が終わったら、ComposingでEnterを押して全体を確定します。各部分変換時に個別の学習が記録されるほか、最終確定時に元のひらがな全文→最終結果の学習も記録されます。

- Shift+Left/Right: 1文字ずつ選択範囲を拡大/縮小
- Shift+Home/End: 先頭/末尾まで一括選択
- 通常の矢印キー: カーソル移動（選択解除）
- 対象: `karukan-im/src/core/engine/input_buffer.rs`, `cursor.rs`, `display.rs`, `input.rs`, `conversion.rs`, `mod.rs`

#### 4. F6〜F10による直接変換
Composing状態および変換中にファンクションキーで直接変換できます。MS-IME/ATOKと同様のキーバインドです。

| キー | 変換内容 |
|------|----------|
| F6 | ひらがな |
| F7 | 全角カタカナ |
| F8 | 半角カタカナ |
| F9 | 全角英数 |
| F10 | 半角英数 |

- 対象: `karukan-im/src/core/engine/conversion.rs`, `input.rs`, `karukan-engine/src/kana.rs`, `lib.rs`

#### 5. 変換候補の冗長フィルタリング
モデル推論結果のうち、出力文字数が読みの文字数を超える候補を除外します。また、Space変換時の学習キャッシュ候補は完全一致のみに限定し、前方一致（予測）候補を除外します。これにより「へんかん」→「変換ウインドウ」のような冗長な候補が表示されなくなります。

- 対象: `karukan-im/src/core/engine/conversion.rs`

#### 6. 記号の全角変換
ひらがなモード時に `#`, `(`, `)`, `@`, `<`, `>` 等の記号が全角に変換されます。

- 対象: `karukan-engine/src/romaji/rules.rs`

### 設定オプション

`~/.config/karukan-im/config.toml` に以下の設定を追加しています。

```toml
[conversion]
# 入力中の自動変換候補表示を無効化（false = Spaceキー変換時のみ表示）
auto_suggest = false

# 変換候補ウインドウを表示するまでのSpace押下回数（0 = 常に表示）
candidate_window_threshold = 3

# 補助テキスト（推論時間・辞書ソース等）の表示（false = 常に非表示）
show_aux_text = false
```

| 設定項目 | デフォルト | 説明 |
|---|---|---|
| `auto_suggest` | `true` | 入力中に変換候補を自動表示する |
| `candidate_window_threshold` | `3` | 候補ウインドウを表示するまでのSpace押下回数。0で常に表示 |
| `show_aux_text` | `true` | 推論時間・辞書ソース等の補助テキストを表示する |

#### 7. コミット時のステートクリーンアップ統一
変換確定・キャンセル・直接変換など、Empty状態に遷移するすべてのパスで共通ヘルパー `enter_empty_state()` を使用するように統一しました。従来は一部のパスで `input_buf` のカーソル位置やセレクション、`live.text`、ローマ字コンバータの状態が適切にリセットされておらず、連続変換時に前回の状態が残留する可能性がありました。

- 対象: `karukan-im/src/core/engine/mod.rs`, `conversion.rs`, `input.rs`

#### 8. reset時のConversion状態コミット保護
fcitx5がアプリ側のイベント等で `reset()` を呼んだ際、従来はComposing/Conversion状態を問わず変換中のテキストを破棄していました。これにより変換候補が確定されないまま消失し、直前にコミット済みの文字列が画面上に残ることで「前の変換結果が出力される」ように見えるバグが発生していました。Conversion状態（ユーザーが明示的にSpaceで変換を開始し候補を選択中）に限り、reset時に選択中の候補をアプリにコミットしてから状態をクリアするように変更しました。Composing状態（入力途中）は従来通り破棄します。

- 対象: `karukan-im/src/core/engine/mod.rs`, `karukan-im/src/ffi/input.rs`, `karukan-im/fcitx5-addon/src/karukan.cpp`

#### 9. アラビア数字→漢数字の変換候補生成
変換候補のフォールバックとして、入力テキスト中のアラビア数字を一桁ずつ漢数字に置換した候補を自動生成します。「2」→「二」、「312」→「三一二」、「20世紀」→「二〇世紀」のように変換できます。

- 対象: `karukan-engine/src/kana.rs`, `karukan-im/src/core/engine/conversion.rs`

#### 10. テストの修正
フォークによる動作変更（Shift英字後のモード自動復帰、記号の全角変換等）に合わせて、期待値がずれていた15件のテストを修正しました。アルファベットモードの復帰動作、`<`→`＜`の全角変換、モデル未ロード時のaux text空文字列などに対応しています。

- 対象: `karukan-im/src/core/engine/tests/alphabet.rs`, `conversion.rs`, `katakana.rs`, `mode_toggle.rs`, `passthrough.rs`, `karukan-im/src/ffi/tests.rs`

#### 11. Backspace後のローマ字再結合
ローマ字入力中に誤打した文字をBackspaceで削除した後、正しい文字を入力しても変換が効かない問題を修正しました。以下の3つのケースに対応しています。

1. **ローマ字バッファ内のreclaim**: `hs` と打って `s` を削除すると、パススルーされた `h` がバッファに戻り、`a` を入力すれば「は」になります。
2. **「ん」のreclaim**: `ns` と打つと `n` が自動的に「ん」に変換されますが、`s` を削除すると「ん」が `n` に戻り、`a` を入力すれば「な」になります。明示的な `nn` や `n'` による「ん」はreclaimされません。
3. **確定済みひらがな後のreclaim**: `namko`（なmこ）と打った後、「こ」を削除するとパススルーされた `m` がバッファに戻り、`a` を入力すれば「ま」になります。これにより「なまこ」への復帰が可能です。

- 対象: `karukan-engine/src/romaji/converter.rs`, `karukan-im/src/core/engine/cursor.rs`

#### 12. Tab/Downによるオートサジェスト候補の選択
入力中に表示されるオートサジェスト候補（学習キャッシュ・モデル推論・辞書）をTab/Downキーで直接選択できるようにしました。従来はこれらの候補は表示のみで選択不可能でしたが、Tab/Downで候補を選択するとConversion状態に遷移し、Up/Downで移動、Enter/Spaceで確定できます。Spaceキーは従来通りビームサーチによるフル変換を実行するため、用途に応じた使い分けが可能です。

| キー | Composing状態での動作 |
|------|----------------------|
| Space | フル変換（モデル再推論） |
| Tab/Down | オートサジェスト候補を選択（再推論なし） |

- 対象: `karukan-im/src/core/engine/mod.rs`, `input.rs`, `conversion.rs`

### 辞書の拡張

jawiki（Wikipedia固有名詞）のシステム辞書統合や、顔文字・絵文字辞書の導入手順については [辞書セットアップガイド](docs/dictionary-setup.md) を参照してください。
