<div align="center">
  <img src="icon.png" width="128" alt="karukan" />
  <h1>Karukan</h1>
  <p>Linux・macOS向け日本語入力システム — ニューラルかな漢字変換エンジン</p>

[![CI (engine)](https://github.com/togatoga/karukan/actions/workflows/karukan-engine-ci.yml/badge.svg)](https://github.com/togatoga/karukan/actions/workflows/karukan-engine-ci.yml)
[![CI (im)](https://github.com/togatoga/karukan/actions/workflows/karukan-im-ci.yml/badge.svg)](https://github.com/togatoga/karukan/actions/workflows/karukan-im-ci.yml)
[![CI (fcitx5)](https://github.com/togatoga/karukan/actions/workflows/karukan-fcitx5-ci.yml/badge.svg)](https://github.com/togatoga/karukan/actions/workflows/karukan-fcitx5-ci.yml)
[![CI (macos)](https://github.com/togatoga/karukan/actions/workflows/karukan-macos-ci.yml/badge.svg)](https://github.com/togatoga/karukan/actions/workflows/karukan-macos-ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
</div>

<div align="center">
  <img src="images/demo.gif" width="800" alt="karukan demo" />
</div>

## プロジェクト構成

| クレート                            | 説明                                                                                |
| ----------------------------------- | ----------------------------------------------------------------------------------- |
| [karukan-fcitx5](karukan-fcitx5/)   | Linux向けIMEフロントエンド — fcitx5アドオン + C FFI                                 |
| [karukan-macos](karukan-macos/)     | macOS向けIMEフロントエンド — Swift/InputMethodKit                                   |
| [karukan-im](karukan-im/)           | 共有IMEエンジン — ステートマシン、ローマ字変換、karukan-imserver(macOS向けJSON-RPCサーバ) |
| [karukan-engine](karukan-engine/)   | コアライブラリ — ローマ字→ひらがな変換 + llama.cppによるニューラルかな漢字変換      |
| [karukan-cli](karukan-cli/)         | CLIツール・サーバ — 辞書ビルド、Sudachi辞書生成、辞書ビューア、AJIMEE-Bench、HTTPサーバ |

## 特徴

- **ニューラルかな漢字変換**: GPT-2ベースのモデルをllama.cppで推論し、高度な日本語変換
- **ライブ変換**: 入力と同時に変換結果をリアルタイム表示。Spaceを押さずに変換が進む（`Ctrl+Shift+L` でON/OFF）
- **コンテキスト対応**: 周辺テキストを考慮した日本語変換
- **変換学習**: ユーザが選択した変換結果を記憶し、次回以降の変換で優先表示。予測変換（前方一致）にも対応し、入力途中でも学習済みの候補を提示
- **システム辞書**: [SudachiDict](https://github.com/WorksApplications/SudachiDict)の辞書データからシステム辞書を構築
- **候補リライター (Mozcから移植)**: 半角カタカナ、英字の大文字小文字・全角半角、記号の関連候補、数字の各種表記（漢数字・大字・ローマ数字・丸数字・16/8/2進数）を自動生成。各候補にはMozc由来の注釈（「半角カタカナ」「16進数」など）が付く
- **絵文字入力**: かな読み（`ぴえん` → 🥺、`きんにく` → 💪）と Slack 風 `:trigger` クエリ（`:smile` → 😄、`:halo` → 😇）の両方をサポート

> **Note:** 初回起動時にHugging Faceからモデルをダウンロードするため、初回の変換開始までに時間がかかります。2回目以降はダウンロード済みのモデルが使用されます。

## インストール

- **Linux (fcitx5)**: [karukan-fcitx5 の README](karukan-fcitx5/README.md#install) を参照
- **macOS**: [karukan-macos の README](karukan-macos/README.md) を参照

## ライセンス

MIT OR Apache-2.0 のデュアルライセンスで提供しています。

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

[karukan-engine/data/](karukan-engine/data/) 配下には [Mozc](https://github.com/google/mozc)（Google製日本語入力システム）から派生したデータを含み、こちらは [BSD 3-Clause License](http://opensource.org/licenses/BSD-3-Clause) のもとで配布されています。各派生ファイルの由来およびMozcの著作権表記は [THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES) を参照してください。

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

ComposingおよびConversion状態でShift+矢印キーを使って選択範囲を作り、Spaceを押すと選択部分のみを変換します。確定（Enter）すると変換結果がComposingバッファに焼き込まれ（bake）、残りの部分を続けて変換できます。選択方向は自由で、先頭からでも末尾からでも部分変換が可能です。すべての部分変換が終わったら、ComposingでEnterを押して全体を確定します。各部分変換時に個別の学習が記録されるほか、最終確定時に元のひらがな全文→最終結果の学習も記録されます。

**変換済み状態からの部分再変換**: 変換結果が表示されている状態（Conversion状態、またはライブ変換中のComposing状態）でShift+矢印を押すと、変換結果が `input_buf` に焼き込まれ、その中の任意の部分を選択して再変換できます。これにより問題があると思う箇所のみを別の漢字に置き換えられます。

例: 「わたしはぺんです」と入力 → ライブ変換で「私はペンです」が表示 → Shift+矢印で「ペン」を選択 → Space → 「便」に再変換 → 「私は便です」が確定可能。

漢字に変換された部分（例:「ペン」）を選択した場合、元のひらがな読み（「ぺん」）とのアライメントを文字単位で計算し、対応する読みを抽出して再変換します。連続する漢字熟語（例:「会議室」）はサブ部分（「会議」だけ）に分割できず、熟語全体が再変換対象になります。

- Shift+Left/Right: 1文字ずつ選択範囲を拡大/縮小
- Shift+Home/End: 先頭/末尾まで一括選択
- 通常の矢印キー: カーソル移動（選択解除）
- 対象: `karukan-engine/src/align.rs`, `karukan-im/src/core/engine/input_buffer.rs`, `cursor.rs`, `display.rs`, `input.rs`, `conversion.rs`, `mod.rs`

#### 4. F6〜F10による直接変換

Composing状態および変換中にファンクションキーで直接変換できます。MS-IME/ATOKと同様のキーバインドです。

| キー | 変換内容     |
| ---- | ------------ |
| F6   | ひらがな     |
| F7   | 全角カタカナ |
| F8   | 半角カタカナ |
| F9   | 全角英数     |
| F10  | 半角英数     |

- 対象: `karukan-im/src/core/engine/conversion.rs`, `input.rs`, `karukan-engine/src/kana.rs`, `lib.rs`

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

| 設定項目                     | デフォルト | 説明                                                     |
| ---------------------------- | ---------- | -------------------------------------------------------- |
| `auto_suggest`               | `true`     | 入力中に変換候補を自動表示する                           |
| `candidate_window_threshold` | `3`        | 候補ウインドウを表示するまでのSpace押下回数。0で常に表示 |
| `show_aux_text`              | `true`     | 推論時間・辞書ソース等の補助テキストを表示する           |

#### 5. コミット時のステートクリーンアップ統一

変換確定・キャンセル・直接変換など、Empty状態に遷移するすべてのパスで共通ヘルパー `enter_empty_state()` を使用するように統一しました。従来は一部のパスで `input_buf` のカーソル位置やセレクション、`live.text`、ローマ字コンバータの状態が適切にリセットされておらず、連続変換時に前回の状態が残留する可能性がありました。

- 対象: `karukan-im/src/core/engine/mod.rs`, `conversion.rs`, `input.rs`

#### 6. reset時のConversion状態コミット保護

fcitx5がアプリ側のイベント等で `reset()` を呼んだ際、従来はComposing/Conversion状態を問わず変換中のテキストを破棄していました。これにより変換候補が確定されないまま消失し、直前にコミット済みの文字列が画面上に残ることで「前の変換結果が出力される」ように見えるバグが発生していました。Conversion状態（ユーザが明示的にSpaceで変換を開始し候補を選択中）に限り、reset時に選択中の候補をアプリにコミットしてから状態をクリアするように変更しました。Composing状態（入力途中）は従来通り破棄します。

- 対象: `karukan-im/src/core/engine/mod.rs`, `karukan-im/src/ffi/input.rs`, `karukan-im/fcitx5-addon/src/karukan.cpp`

#### 7. テストの修正

フォークによる動作変更（Shift英字後のモード自動復帰、Tab/Downのauto-suggest選択、Shift+Spaceの学習スキップ等）に合わせて、関連するテストを修正しました。

- 対象: `karukan-im/src/core/engine/tests/` 配下、`karukan-im/src/ffi/tests.rs`

#### 8. Backspace後のローマ字再結合

ローマ字入力中に誤打した文字をBackspaceで削除した後、正しい文字を入力しても変換が効かない問題を修正しました。以下の3つのケースに対応しています。

1. **ローマ字バッファ内のreclaim**: `hs` と打って `s` を削除すると、パススルーされた `h` がバッファに戻り、`a` を入力すれば「は」になります。
2. **「ん」のreclaim**: `ns` と打つと `n` が自動的に「ん」に変換されますが、`s` を削除すると「ん」が `n` に戻り、`a` を入力すれば「な」になります。明示的な `nn` や `n'` による「ん」はreclaimされません。
3. **確定済みひらがな後のreclaim**: `namko`（なmこ）と打った後、「こ」を削除するとパススルーされた `m` がバッファに戻り、`a` を入力すれば「ま」になります。これにより「なまこ」への復帰が可能です。

- 対象: `karukan-engine/src/romaji/converter.rs`, `karukan-im/src/core/engine/cursor.rs`

#### 9. Tab/Downによるオートサジェスト候補の選択

入力中に表示されるオートサジェスト候補（学習キャッシュ・モデル推論・辞書）をTab/Downキーで直接選択できるようにしました。従来は表示のみで選択不可能でしたが、Tab/Downで候補を選択するとConversion状態に遷移し、Up/Downで移動、Enter/Spaceで確定できます。

上流ではTabが「学習スキップ変換」に割り当てられていますが、フォークではTabはauto-suggest選択用とし、学習スキップ変換はShift+Spaceに割り当て直しています。

| キー        | Composing状態での動作                            |
| ----------- | ------------------------------------------------ |
| Space       | フル変換（モデル再推論、学習込み）               |
| Shift+Space | 学習スキップ変換（学習キャッシュを使わずに変換） |
| Tab/Down    | オートサジェスト候補を選択（再推論なし）         |

- 対象: `karukan-im/src/core/engine/mod.rs`, `input.rs`, `conversion.rs`

### 辞書の拡張

jawiki（Wikipedia固有名詞）・Mozc OSS辞書（IPAdic由来の副助詞・接尾辞・連体詞補完、例: `など → 等`）のシステム辞書統合や、顔文字・絵文字辞書・記号辞書の導入手順については [辞書セットアップガイド](docs/dictionary-setup.md) を参照してください。
