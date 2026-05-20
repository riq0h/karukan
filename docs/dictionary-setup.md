# 辞書セットアップガイド

karukan-imの辞書を拡張するための辞書導入手順です。

## 概要

| 辞書                        | エントリ数 | 用途                                   |
| --------------------------- | ---------- | -------------------------------------- |
| システム辞書（SudachiDict） | 約266万    | 標準の漢字変換（要ダウンロード）       |
| jawiki辞書                  | 約71万     | Wikipedia由来の固有名詞・専門用語      |
| Mozc OSS辞書                | 約42万     | IPAdic由来の副助詞・接尾辞・連体詞等   |
| 顔文字辞書                  | 約6000     | ASCII顔文字（`(^o^)` 等）              |

> **Note:** 記号（`…`、括弧バリエーション等）とUnicode絵文字（😊、🥺等）は本体に同梱の SymbolRewriter / EmojiRewriter が自動生成するため、ユーザ辞書は不要です。「ぴえん」→ 🥺、`:smile` → 😊、「かっこ」→ 各種括弧、「さんてんりーだ」→ `…` などが既定で機能します。

## 0. システム辞書のダウンロード

jawiki統合のベースとなるシステム辞書を取得します。すでに `~/.local/share/karukan-im/dict.bin` が存在する場合はスキップしてください。

```bash
wget https://github.com/togatoga/karukan/releases/download/v0.1.0/dict.tgz
tar xzf dict.tgz
mkdir -p ~/.local/share/karukan-im
cp dict.bin ~/.local/share/karukan-im/
rm dict.tgz dict.bin
```

## 1. jawiki辞書のシステム辞書への統合

jawikiエントリをシステム辞書に統合することで、別辞書ロードのオーバーヘッドなく語彙を拡張できます。

### 1.1 Mozc UT辞書のダウンロード

```bash
# jawiki辞書（Mozc UT形式）をクローン
cd /tmp
git clone --depth 1 https://github.com/utuhiro78/mozcdic-ut-jawiki.git
bzip2 -dk mozcdic-ut-jawiki/mozcdic-ut-jawiki.txt.bz2
```

### 1.2 jawikiエントリのフィルタリング

Mozc UT形式（5カラム）からkarukan形式（4カラム）に変換し、読みと表記が同一のエントリを除外します。

```bash
python3 << 'PYEOF' > /tmp/jawiki-filtered.tsv
import sys
seen = set()
count = 0
with open("/tmp/mozcdic-ut-jawiki/mozcdic-ut-jawiki.txt") as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) < 5:
            continue
        reading, word = parts[0], parts[4]
        if reading == word or not reading or not word:
            continue
        key = (reading, word)
        if key in seen:
            continue
        seen.add(key)
        print(f"{reading}\t{word}\t名詞\t")
        count += 1
print(f"# Output: {count} entries", file=sys.stderr)
PYEOF
```

### 1.3 システム辞書との重複除去

```bash
# 現在のシステム辞書をダンプ（karukanリポジトリのルートから実行）
cargo run --release --bin karukan-dict -- view ~/.local/share/karukan-im/dict.bin --all > /tmp/sysdict-dump.tsv

# 重複を除去
python3 << 'PYEOF' > /tmp/jawiki-deduped.tsv
import sys

sysdict = set()
with open("/tmp/sysdict-dump.tsv") as f:
    for line in f:
        parts = line.split('\t')
        if len(parts) >= 2:
            sysdict.add((parts[0], parts[1]))

count = kept = 0
with open("/tmp/jawiki-filtered.tsv") as f:
    for line in f:
        line = line.rstrip('\n')
        parts = line.split('\t')
        if len(parts) < 2:
            continue
        count += 1
        if (parts[0], parts[1]) not in sysdict:
            kept += 1
            print(line)
print(f"Input: {count}, Kept: {kept}, Removed: {count - kept}", file=sys.stderr)
PYEOF
```

### 1.4 Mozc OSS辞書のダウンロード

SudachiDictにはカバーされていない副助詞・接尾辞・連体詞・準体助詞などのIPAdic由来の基礎語彙を補完します。たとえば「など → 等」はSudachiDictに存在しないため、Mozc OSS辞書から取得します。

```bash
# Mozc OSS辞書 (dictionary00.txt〜dictionary09.txt) のダウンロード
mkdir -p /tmp/mozc-oss-dict
cd /tmp/mozc-oss-dict
for i in 00 01 02 03 04 05 06 07 08 09; do
    curl -sL -o "dictionary${i}.txt" \
      "https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss/dictionary${i}.txt" &
done
wait
```

### 1.5 Mozc OSSエントリのフィルタリング

Mozc OSS形式（5カラム: `読み\tlid\trid\tコスト\t表記`）から `読み\t表記\tコスト` の3カラムTSVに変換し、ひらがな読みのみ抽出します。同一の `(読み, 表記)` ペアは最初の出現を残します。

```bash
python3 << 'PYEOF' > /tmp/mozc-oss-filtered.tsv
import sys, glob
seen = set()
count = 0
for path in sorted(glob.glob('/tmp/mozc-oss-dict/dictionary*.txt')):
    with open(path) as f:
        for line in f:
            parts = line.rstrip('\n').split('\t')
            if len(parts) < 5:
                continue
            reading, _lid, _rid, cost, surface = parts[:5]
            if not reading or not surface or reading == surface:
                continue
            # ひらがな読みのみ（ーを許可）
            if not all('぀' <= c <= 'ゟ' or c == 'ー' for c in reading):
                continue
            try:
                cost_int = int(cost)
            except ValueError:
                continue
            key = (reading, surface)
            if key in seen:
                continue
            seen.add(key)
            print(f"{reading}\t{surface}\t{cost_int}")
            count += 1
print(f"# Output: {count} entries", file=sys.stderr)
PYEOF
```

### 1.6 既存辞書との重複除去

```bash
python3 << 'PYEOF' > /tmp/mozc-oss-deduped.tsv
import sys
sysdict = set()
with open("/tmp/sysdict-dump.tsv") as f:
    for line in f:
        parts = line.split('\t')
        if len(parts) >= 2:
            sysdict.add((parts[0], parts[1]))

count = kept = 0
with open("/tmp/mozc-oss-filtered.tsv") as f:
    for line in f:
        line = line.rstrip('\n')
        parts = line.split('\t')
        if len(parts) < 3:
            continue
        count += 1
        if (parts[0], parts[1]) not in sysdict:
            kept += 1
            print(line)
print(f"Input: {count}, Kept: {kept}, Removed: {count - kept}", file=sys.stderr)
PYEOF
```

### 1.7 統合辞書のビルド

```bash
# システム辞書ダンプ + jawiki + Mozc OSS をJSON形式に統合
python3 << 'PYEOF'
import json, sys
from collections import OrderedDict

entries = OrderedDict()
sys_pairs = set()

# システム辞書の読み込み（スコア付き）
with open("/tmp/sysdict-dump.tsv") as f:
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 3:
            continue
        reading, surface = parts[0], parts[1]
        try:
            score = float(parts[2])
        except ValueError:
            continue
        entries.setdefault(reading, []).append({"surface": surface, "score": score})
        sys_pairs.add((reading, surface))

# jawikiエントリの追加（スコア6000）
with open("/tmp/jawiki-deduped.tsv") as f:
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 2 or not parts[0] or not parts[1]:
            continue
        if (parts[0], parts[1]) not in sys_pairs:
            entries.setdefault(parts[0], []).append({"surface": parts[1], "score": 6000.0})
            sys_pairs.add((parts[0], parts[1]))

# Mozc OSSエントリの追加（コストをそのままスコアとして使用）
with open("/tmp/mozc-oss-deduped.tsv") as f:
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 3 or not parts[0] or not parts[1]:
            continue
        try:
            cost = float(parts[2])
        except ValueError:
            continue
        if (parts[0], parts[1]) not in sys_pairs:
            entries.setdefault(parts[0], []).append({"surface": parts[1], "score": cost})

result = [{"reading": r, "candidates": c} for r, c in entries.items()]
with open("/tmp/merged-dict.json", "w") as f:
    json.dump(result, f, ensure_ascii=False)
print(f"Entries: {len(result)}", file=sys.stderr)
PYEOF

# バックアップと再ビルド（karukanリポジトリのルートから実行）
cp ~/.local/share/karukan-im/dict.bin ~/.local/share/karukan-im/dict.bin.backup
cargo run --release --bin karukan-dict -- build /tmp/merged-dict.json -o ~/.local/share/karukan-im/dict.bin
```

### 1.8 確認

```bash
cargo run --release --bin karukan-dict -- view -q など ~/.local/share/karukan-im/dict.bin
```

期待される出力（`など → 等` がスコア2077で含まれる）:

```
など	ナド	0
など	等	2077
など	など	5291
など	奈土	9219
など	抔	9433
```

## 2. 顔文字辞書の導入

ASCII顔文字（`(^o^)`、`＼(^o^)／` 等）の辞書を導入します。Unicode絵文字（😊、🥺）は本体同梱の EmojiRewriter で扱われるため、ここでは顔文字のみを対象とします。

### 2.1 ソースのダウンロードと変換

```bash
# 各辞書のクローン
cd /tmp
git clone --depth 1 https://github.com/6/kaomoji-json.git
git clone --depth 1 https://github.com/tiwanari/emoticon.git
curl -sL -o mozc-emoticon.tsv \
  https://raw.githubusercontent.com/google/mozc/master/src/data/emoticon/emoticon.tsv

# 統合スクリプト（ASCII顔文字のみ抽出）
python3 << 'PYEOF' > /tmp/kaomoji.tsv
import json, sys

seen = set()
count = 0

def emit(reading, word):
    global seen, count
    reading, word = reading.strip(), word.strip()
    if not reading or not word or reading == word:
        return
    if (reading, word) in seen:
        return
    seen.add((reading, word))
    print(f"{reading}\t{word}\t顔文字\t")
    count += 1

# 6/kaomoji-json
try:
    for entry in json.load(open("/tmp/kaomoji-json/kao-utf8.json")):
        emit(entry.get("annotation", ""), entry.get("face", ""))
except FileNotFoundError:
    pass

# tiwanari/emoticon
try:
    for line in open("/tmp/emoticon/emoticon.txt"):
        parts = line.rstrip("\n").split("\t")
        if len(parts) >= 3:
            emit(parts[0].lstrip("@"), parts[1])
except FileNotFoundError:
    pass

# Mozc built-in emoticon
try:
    for line in open("/tmp/mozc-emoticon.tsv"):
        if line.startswith("keys"):
            continue
        parts = line.rstrip("\n").split("\t")
        if len(parts) >= 2:
            for reading in parts[1].split():
                emit(reading, parts[0])
except FileNotFoundError:
    pass

print(f"# Total: {count} entries", file=sys.stderr)
PYEOF
```

### 2.2 ユーザ辞書として配置

```bash
mkdir -p ~/.local/share/karukan-im/user_dicts
cp /tmp/kaomoji.tsv ~/.local/share/karukan-im/user_dicts/
```

### 2.3 確認

fcitx5を再起動して動作を確認します。

```bash
fcitx5 -r -d
```

テスト入力例:

- 「にこにこ」→ `＼(^o^)／`
- 「えがお」→ `😀`（本体のEmojiRewriterが生成）
- `:smile` → 😄（Slack風 `:trigger` 入力、本体のEmojiRewriterが生成）

## 辞書ソース

| 辞書                | ライセンス                   | URL                                                                |
| ------------------- | ---------------------------- | ------------------------------------------------------------------ |
| mozcdic-ut-jawiki   | Apache-2.0                   | https://github.com/utuhiro78/mozcdic-ut-jawiki                     |
| Mozc OSS dictionary | BSD-3-Clause + NAIST/ICOT/PD | https://github.com/google/mozc/tree/master/src/data/dictionary_oss |
| kaomoji-json        | -                            | https://github.com/6/kaomoji-json                                  |
| tiwanari/emoticon   | MIT                          | https://github.com/tiwanari/emoticon                               |
| Mozc emoticon.tsv   | BSD-3-Clause                 | https://github.com/google/mozc                                     |

> **Note:** Mozc symbol.tsv と emoji_data.tsv は本体に同梱（`karukan-engine/data/symbols.yml`、`emoji.yml`）されており、ユーザ辞書としての導入は不要です。
