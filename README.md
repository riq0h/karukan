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

IME本体(コアエンジン + 各プラットフォームのフロントエンド)は `karukan-im/` 配下にまとまっています。

- [karukan-im/](karukan-im/) — IME本体
  - [core/](karukan-im/core/) — 共有IMEエンジン(crate: `karukan-im`) — ステートマシン、ローマ字変換、karukan-imserver(macOS向けJSON-RPCサーバ)
  - [fcitx5/](karukan-im/fcitx5/) — Linux向けフロントエンド(crate: `karukan-fcitx5`) — fcitx5アドオン + C FFI
  - [macos/](karukan-im/macos/) — macOS向けフロントエンド — Swift/InputMethodKit
- [karukan-engine/](karukan-engine/) — コアライブラリ — ローマ字→ひらがな変換 + llama.cppによるニューラルかな漢字変換
- [karukan-cli/](karukan-cli/) — CLIツール・サーバ — 辞書ビルド、Sudachi辞書生成、辞書ビューア、AJIMEE-Bench、HTTPサーバ

## 特徴

- **ニューラルかな漢字変換**: GPT-2/Qwen3ベースのモデルをllama.cppで推論し、高度な日本語変換
- **ライブ変換**: 入力と同時に変換結果をリアルタイム表示。Spaceを押さずに変換が進む（`Ctrl+Shift+L` でON/OFF）
- **コンテキスト対応**: 周辺テキストを考慮した日本語変換
- **変換学習**: ユーザが選択した変換結果を記憶し、次回以降の変換で優先表示。予測変換（前方一致）にも対応し、入力途中でも学習済みの候補を提示
- **システム辞書**: [SudachiDict](https://github.com/WorksApplications/SudachiDict)の辞書データからシステム辞書を構築
- **候補リライター (Mozcから移植)**: 半角カタカナ、英字の大文字小文字・全角半角、記号の関連候補、数字の各種表記（漢数字・大字・ローマ数字・丸数字・16/8/2進数）を自動生成。各候補にはMozc由来の注釈（「半角カタカナ」「16進数」など）が付く
- **絵文字入力**: かな読み（`ぴえん` → 🥺、`きんにく` → 💪）と Slack 風 `:trigger` クエリ（`:smile` → 😄、`:halo` → 😇）の両方をサポート

> **Note:** 初回起動時にHugging Faceからモデルをダウンロードするため、初回の変換開始までに時間がかかります。2回目以降はダウンロード済みのモデルが使用されます。

## インストール

- **Linux (fcitx5)**: [karukan-fcitx5 の README](karukan-im/fcitx5/README.md#install) を参照
- **macOS**: [karukan-macos の README](karukan-im/macos/README.md) を参照

## ドキュメント

- [キーバインド一覧](docs/key-bindings.md) — 共通キーバインドと Linux / macOS 固有キー
- [設定](docs/configuration.md) — config.toml の設定項目、ライブ変換、変換ストラテジー、学習キャッシュ
- [辞書](docs/dictionary.md) — システム辞書のインストール、ユーザ辞書、候補の優先順位
- [ユーザ辞書](docs/user-dictionary.md) — 対応形式（Mozc/Google IME TSV・バイナリ）と登録方法
- [Chunk](docs/chunking.md) — 変換が Chunk に区切られる場所と、自分で区切って表示を固定する方法

## ライセンス

MIT OR Apache-2.0 のデュアルライセンスで提供しています。

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

[karukan-engine/data/](karukan-engine/data/) 配下には [Mozc](https://github.com/google/mozc)（Google製日本語入力システム）から派生したデータを含み、こちらは [BSD 3-Clause License](http://opensource.org/licenses/BSD-3-Clause) のもとで配布されています。各派生ファイルの由来およびMozcの著作権表記は [THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES) を参照してください。

---

## フォーク変更点

本フォークでは、私的な使い勝手を向上させるため以下の変更を加えています。

### 1. 末尾「n」の自動変換

確定・変換時に末尾の `n` を自動的に「ん」に変換します。`nn` と入力する必要がなくなりました（`karukan` → 「かるかん」）。明示的な `nn` や `n'` は従来通り動作します。

- 対象: `karukan-engine/src/romaji/converter.rs`

### 2. F6〜F10による直接変換

Composing状態および変換中にファンクションキーで直接変換・確定できます。MS-IME/ATOKと同様のキーバインドです。変換中に押した場合は選択中の候補ではなく**読み**が変換対象になります。

| キー | 変換内容     |
| ---- | ------------ |
| F6   | ひらがな     |
| F7   | 全角カタカナ |
| F8   | 半角カタカナ |
| F9   | 全角英数     |
| F10  | 半角英数     |

F9/F10 は読みに含まれる ASCII が対象です。打鍵要素列モデルでは確定したかなから元のローマ字を復元できないため、Shift+英字で直接入力したラテン文字が主な対象になります。

- 対象: `karukan-im/core/src/core/engine/direct.rs`, `core/keycode.rs`

### 3. Shift+矢印キーによる選択ベース部分変換

Shift+矢印キーで選択範囲を作り、Spaceを押すと選択部分のみを変換します。確定（Enter）すると変換結果がComposingバッファに焼き込まれ（bake）、残りの部分を続けて変換できます。選択方向は自由で、先頭からでも末尾からでも部分変換が可能です。すべての部分変換が終わったら、ComposingでEnterを押して全体を確定します。

**変換済み状態からの部分再変換**: 変換結果が表示されている状態（Conversion状態、またはライブ変換中のComposing状態）でShift+矢印を押すと、変換結果がバッファに焼き込まれ、その中の任意の部分を選択して再変換できます。問題があると思う箇所のみを別の漢字に置き換えられます。

例: 「わたしはぺんです」と入力 → ライブ変換で「私はペンです」が表示 → Shift+矢印で「ペン」を選択 → Space → 「便」に再変換 → 「私は便です」が確定可能。

漢字・カタカナに変換された部分を選択した場合、元のひらがな読みとのアライメントを文字単位で計算し、対応する読みを抽出して再変換します。連続する漢字熟語（例:「会議室」）はサブ部分（「会議」だけ）に分割できず、熟語全体が再変換の単位になります。

各部分変換時に個別の学習が記録されるほか、最終確定時に元のひらがな全文→最終結果の学習も記録されます。

- Shift+Left/Right: 1文字ずつ選択範囲を拡大/縮小
- Shift+Home/End: 先頭/末尾まで一括選択
- 通常の矢印キー: カーソル移動（選択解除）
- 対象: `karukan-engine/src/align.rs`, `karukan-im/core/src/core/engine/partial.rs`, `input_buffer.rs`, `cursor.rs`, `display.rs`

### 4. Tab/Downによるオートサジェスト候補の選択

入力中に表示されるオートサジェスト候補（学習キャッシュ・モデル推論・辞書）をTab/Downキーで直接選択できます。画面に出ている候補をそのまま変換状態へ引き上げるため、モデルの再推論が走りません。Up/Downで移動、Enterで確定できます。

上流ではTabが「学習スキップ変換」に割り当てられていますが、本フォークではTabをサジェスト選択に使い、学習スキップ変換はShift+Spaceに移しています。

| キー        | Composing状態での動作                            |
| ----------- | ------------------------------------------------ |
| Space       | フル変換（モデル再推論、学習込み）               |
| Shift+Space | 学習スキップ変換（学習キャッシュを使わずに変換） |
| Tab / Down  | オートサジェスト候補を選択（再推論なし）         |

- 対象: `karukan-im/core/src/core/engine/conversion.rs`, `input.rs`

### 5. 補助テキストの完全非表示

`[display]` セクションに `show_aux_text` を追加しました。上流の `verbose` は詳細情報（推論時間・モデル名・文脈）の有無を切り替えるもので、`false` でもモード表示や読みは残ります。`show_aux_text = false` はそれらも含めて補助テキストを一切表示しません。モード切替やライブ変換トグルの通知も抑止されます。

```toml
[display]
# 詳細（推論時間・モデル名・文脈）を表示する。Ctrl+Shift+V で実行時に切替可能
verbose = false
# 補助テキストそのものを表示する。false で完全非表示
show_aux_text = false
```

| 設定            | 補助テキストの表示内容                                |
| --------------- | ----------------------------------------------------- |
| `verbose = true`  | `[あ] かんじ Karukan (jinen-v2-small) \| lctx: …`     |
| `verbose = false` | `[あ] かんじ`                                         |
| `show_aux_text = false` | （何も表示しない）                              |

- 対象: `karukan-im/core/src/config/settings.rs`, `core/engine/display.rs`, `mode.rs`

### 辞書の拡張

jawiki（Wikipedia固有名詞）・Mozc OSS辞書（IPAdic由来の副助詞・接尾辞・連体詞補完、例: `など → 等`）のシステム辞書統合や、顔文字辞書の導入手順については [辞書セットアップガイド](docs/dictionary-setup.md) を参照してください。
