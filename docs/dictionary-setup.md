# 辞書セットアップガイド

karukan-imの辞書を拡張するための辞書導入手順です。

## 概要

| 辞書                        | エントリ数 | 用途                                   |
| --------------------------- | ---------- | -------------------------------------- |
| システム辞書（SudachiDict） | 約266万    | 標準の漢字変換（プリインストール済み） |
| jawiki辞書                  | 約71万     | Wikipedia由来の固有名詞・専門用語      |
| 顔文字・絵文字辞書          | 約1.3万    | 顔文字・Unicode絵文字                  |
| 記号辞書（Mozc symbol）     | 約4800     | 特殊記号・括弧・矢印・数学記号等       |

## 1. jawiki辞書のシステム辞書への統合

jawikiエントリをシステム辞書に統合することで、別辞書ロードのオーバーヘッドなく語彙を拡張できます。

### 1.1 Mozc UT辞書のダウンロード

```bash
# jawiki辞書（Mozc UT形式）をダウンロード
# https://github.com/utuhiro78/mozcdic-ut-jawiki/releases から最新版を取得
curl -L -o /tmp/mozcdic-ut-jawiki.tar.bz2 \
  "https://github.com/utuhiro78/mozcdic-ut-jawiki/releases/download/20250315/mozcdic-ut-jawiki-20250315.tar.bz2"
cd /tmp && tar xjf mozcdic-ut-jawiki.tar.bz2
```

### 1.2 jawikiエントリのフィルタリング

Mozc UT形式（5カラム）からkarukan形式（4カラム）に変換し、読みと表記が同一のエントリを除外します。

```bash
python3 << 'EOF' < /tmp/mozcdic-ut-jawiki-*/mozcdic-ut-jawiki.txt > /tmp/jawiki-filtered.tsv
import sys
seen = set()
count = 0
for line in sys.stdin:
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
EOF
```

### 1.3 システム辞書との重複除去

```bash
# 現在のシステム辞書をダンプ
karukan-dict view ~/.local/share/karukan-im/dict.bin --all > /tmp/sysdict-dump.tsv

# 重複を除去
python3 << 'EOF' < /tmp/jawiki-filtered.tsv > /tmp/jawiki-deduped.tsv
import subprocess, sys

result = subprocess.run(
    ["karukan-dict", "view", "$HOME/.local/share/karukan-im/dict.bin", "--all"],
    capture_output=True, text=True
)

sysdict = set()
for line in result.stdout.splitlines():
    parts = line.split('\t')
    if len(parts) >= 2:
        sysdict.add((parts[0], parts[1]))

count = kept = 0
for line in sys.stdin:
    line = line.rstrip('\n')
    parts = line.split('\t')
    if len(parts) < 2:
        continue
    count += 1
    if (parts[0], parts[1]) not in sysdict:
        kept += 1
        print(line)
print(f"Input: {count}, Kept: {kept}, Removed: {count - kept}", file=sys.stderr)
EOF
```

### 1.4 統合辞書のビルド

```bash
# システム辞書ダンプ（reading\tword\tscore）とjawikiエントリ（reading\tword\tPOS\t）を
# JSON形式に統合してビルド
python3 << 'PYEOF' > /tmp/merged-dict.json
import json
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

result = [{"reading": r, "candidates": c} for r, c in entries.items()]
json.dump(result, open("/tmp/merged-dict.json", "w"), ensure_ascii=False)
PYEOF

# バックアップと再ビルド
cp ~/.local/share/karukan-im/dict.bin ~/.local/share/karukan-im/dict.bin.backup
karukan-dict build /tmp/merged-dict.json -o ~/.local/share/karukan-im/dict.bin
```

## 2. 顔文字・絵文字辞書の導入

軽量なTSV形式でユーザー辞書ディレクトリに配置します。

### 2.1 ソースのダウンロードと変換

```bash
# 各辞書のクローン
cd /tmp
git clone --depth 1 https://github.com/peaceiris/emoji-ime-dictionary.git
git clone --depth 1 https://github.com/6/kaomoji-json.git
git clone --depth 1 https://github.com/tiwanari/emoticon.git
curl -sL -o mozc-emoticon.tsv \
  https://raw.githubusercontent.com/google/mozc/master/src/data/emoticon/emoticon.tsv

# 統合スクリプト
python3 << 'EOF' > /tmp/kaomoji-emoji.tsv
import json, sys

seen = set()
count = 0

def emit(reading, word, pos="顔文字"):
    global seen, count
    reading, word = reading.strip(), word.strip()
    if not reading or not word or reading == word:
        return
    if (reading, word) in seen:
        return
    seen.add((reading, word))
    print(f"{reading}\t{word}\t{pos}\t")
    count += 1

# peaceiris/emoji-ime-dictionary
for fname in ["/tmp/emoji-ime-dictionary/tsv/emoji.tsv",
              "/tmp/emoji-ime-dictionary/tsv/emoji_additional.tsv"]:
    try:
        for line in open(fname):
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 3:
                emit(parts[0].lstrip(":"), parts[1], "記号")
    except FileNotFoundError:
        pass

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
EOF
```

### 2.2 ユーザー辞書として配置

```bash
mkdir -p ~/.local/share/karukan-im/user_dicts
cp /tmp/kaomoji-emoji.tsv ~/.local/share/karukan-im/user_dicts/
```

### 2.3 確認

fcitx5を再起動して動作を確認します。

```bash
fcitx5 -r -d
```

テスト入力例:

- 「にこにこ」→ `＼(^o^)／`
- 「えがお」→ `😀`
- 「はーと」→ `❤`

## 3. 記号辞書の導入

Mozc由来の記号辞書（約4800エントリ）を導入し、三点リーダ（…）、各種括弧（【】『』〈〉等）、ダッシュ（—）、矢印（→←↑↓）、数学記号（×÷±≠）、米印（※）などをひらがな読みから変換できるようにします。

### 3.1 ダウンロードと変換

```bash
# Mozc記号辞書のダウンロード
curl -sL -o /tmp/mozc-symbol.tsv \
  https://raw.githubusercontent.com/google/mozc/master/src/data/symbol/symbol.tsv

# karukan用Mozc TSV形式に変換（ひらがな読みのみ抽出）
python3 << 'EOF' > /tmp/symbol-dict.tsv
import sys

seen = set()
count = 0

def is_hiragana(s):
    return all('\u3040' <= c <= '\u309f' or c == '\u30fc' for c in s) and len(s) > 0

with open("/tmp/mozc-symbol.tsv", encoding="utf-8") as f:
    for line in f:
        line = line.rstrip("\n")
        if not line or line.startswith("POS"):
            continue
        parts = line.split("\t")
        if len(parts) < 4:
            continue
        char, readings_str, description = parts[1], parts[2], parts[3]
        if not char or not readings_str:
            continue
        for reading in readings_str.split():
            reading = reading.strip()
            if not is_hiragana(reading):
                continue
            if (reading, char) in seen:
                continue
            seen.add((reading, char))
            print(f"{reading}\t{char}\t記号\t{description}")
            count += 1

print(f"# Total: {count} entries", file=sys.stderr)
EOF
```

### 3.2 括弧の類似変換エントリ追加

Mozc記号辞書の括弧はペア（「」、『』）のみで、片方だけの入力から別種の括弧へ変換できません。以下のコマンドで「「」→「『」「【」「〈」等のバリエーション候補を追加します。

```bash
cat >> /tmp/symbol-dict.tsv << 'EOF'
「	『	記号	二重かぎ括弧(開)
「	【	記号	すみつき括弧(開)
「	〈	記号	山括弧(開)
「	《	記号	二重山括弧(開)
「	〔	記号	甲括弧(開)
「	［	記号	角括弧(開)
「	｛	記号	波括弧(開)
「	（	記号	丸括弧(開)
「	"	記号	ダブル引用符(開)
「	'	記号	シングル引用符(開)
」	』	記号	二重かぎ括弧(閉)
」	】	記号	すみつき括弧(閉)
」	〉	記号	山括弧(閉)
」	》	記号	二重山括弧(閉)
」	〕	記号	甲括弧(閉)
」	］	記号	角括弧(閉)
」	｝	記号	波括弧(閉)
」	）	記号	丸括弧(閉)
」	"	記号	ダブル引用符(閉)
」	'	記号	シングル引用符(閉)
『	「	記号	かぎ括弧(開)
『	【	記号	すみつき括弧(開)
『	〈	記号	山括弧(開)
『	《	記号	二重山括弧(開)
』	」	記号	かぎ括弧(閉)
』	】	記号	すみつき括弧(閉)
』	〉	記号	山括弧(閉)
』	》	記号	二重山括弧(閉)
（	〔	記号	甲括弧(開)
（	｛	記号	波括弧(開)
（	［	記号	角括弧(開)
（	〈	記号	山括弧(開)
（	《	記号	二重山括弧(開)
（	「	記号	かぎ括弧(開)
（	『	記号	二重かぎ括弧(開)
（	【	記号	すみつき括弧(開)
）	〕	記号	甲括弧(閉)
）	｝	記号	波括弧(閉)
）	］	記号	角括弧(閉)
）	〉	記号	山括弧(閉)
）	》	記号	二重山括弧(閉)
）	」	記号	かぎ括弧(閉)
）	』	記号	二重かぎ括弧(閉)
）	】	記号	すみつき括弧(閉)
【	「	記号	かぎ括弧(開)
【	『	記号	二重かぎ括弧(開)
【	〈	記号	山括弧(開)
【	《	記号	二重山括弧(開)
】	」	記号	かぎ括弧(閉)
】	』	記号	二重かぎ括弧(閉)
】	〉	記号	山括弧(閉)
】	》	記号	二重山括弧(閉)
「	〝	記号	爪括弧(開)
」	〟	記号	爪括弧(閉)
『	〝	記号	爪括弧(開)
』	〟	記号	爪括弧(閉)
EOF
```

### 3.3 記号の類似変換エントリ追加

ローマ字ルールにより `=` → `＝`、`<` → `＜` 等の全角記号に変換されますが、Space変換時に関連記号（`≒`、`≠`、`≡` 等）を候補に表示するためのエントリを追加します。readingに全角記号そのものを指定することで、変換時に辞書検索で類似記号が候補に表示されます。

```bash
cat >> /tmp/symbol-dict.tsv << 'EOF'
＝	≠	記号	等しくない
＝	≒	記号	ほぼ等しい
＝	≡	記号	合同・恒等
＝	≢	記号	合同でない
＝	≈	記号	近似的に等しい
＝	≅	記号	合同(幾何)
＝	≃	記号	漸近的に等しい
＝	≔	記号	定義(コロンイコール)
＝	≝	記号	定義に等しい
＝	∝	記号	比例
＜	≦	記号	小なりイコール
＜	≤	記号	小なりイコール(ISO)
＜	≪	記号	非常に小さい
＜	≮	記号	小さくない
＜	≲	記号	小さいか類似
＜	⟨	記号	数学山括弧(開)
＜	‹	記号	ギュメ(開)
＞	≧	記号	大なりイコール
＞	≥	記号	大なりイコール(ISO)
＞	≫	記号	非常に大きい
＞	≯	記号	大きくない
＞	≳	記号	大きいか類似
＞	⟩	記号	数学山括弧(閉)
＞	›	記号	ギュメ(閉)
＋	±	記号	プラスマイナス
＋	∓	記号	マイナスプラス
＋	⊕	記号	丸付きプラス
＊	※	記号	米印・注釈
＊	✱	記号	太アスタリスク
＊	✳	記号	八芒アスタリスク
＊	★	記号	黒星
＊	☆	記号	白星
＊	✦	記号	黒四芒星
＊	✧	記号	白四芒星
＃	♯	記号	シャープ(音楽)
＃	♭	記号	フラット(音楽)
＃	♮	記号	ナチュラル(音楽)
＃	♪	記号	音符
＃	♫	記号	連続音符
？	⁇	記号	二重疑問符
？	‽	記号	疑問感嘆符
？	⁈	記号	疑問感嘆
？	¿	記号	逆疑問符
！	‼	記号	二重感嘆符
！	⁉	記号	感嘆疑問
！	¡	記号	逆感嘆符
％	‰	記号	パーミル(千分率)
％	‱	記号	パーミリアド(万分率)
｜	‖	記号	二重縦線
｜	¦	記号	破断縦線
｜	│	記号	罫線(細)
｜	┃	記号	罫線(太)
・	·	記号	中点(Latin)
・	•	記号	ビュレット
・	∙	記号	ビュレット演算子
・	⋅	記号	ドット演算子
・	◦	記号	白丸ビュレット
・	‧	記号	ハイフネーションポイント
＄	¢	記号	セント
＄	£	記号	ポンド
＄	¥	記号	円・元
＄	€	記号	ユーロ
＄	₩	記号	ウォン
＄	₹	記号	ルピー
〜	～	記号	全角チルダ
〜	∼	記号	チルダ演算子
〜	≈	記号	近似
EOF
```

### 3.4 ユーザー辞書として配置

```bash
mkdir -p ~/.local/share/karukan-im/user_dicts
cp /tmp/symbol-dict.tsv ~/.local/share/karukan-im/user_dicts/
```

### 3.5 確認

fcitx5を再起動して動作を確認します。

```bash
fcitx5 -r -d
```

テスト入力例:

- 「さんてんりーだ」→ `…`
- 「かっこ」→ `【】`「」『』〈〉
- 「やじるし」→ `→` `←` `↑` `↓`
- 「こめじるし」→ `※`
- 「だっしゅ」→ `—`

## 辞書ソース

| 辞書                 | ライセンス   | URL                                               |
| -------------------- | ------------ | ------------------------------------------------- |
| mozcdic-ut-jawiki    | Apache-2.0   | https://github.com/utuhiro78/mozcdic-ut-jawiki    |
| emoji-ime-dictionary | MIT          | https://github.com/peaceiris/emoji-ime-dictionary |
| kaomoji-json         | -            | https://github.com/6/kaomoji-json                 |
| tiwanari/emoticon    | MIT          | https://github.com/tiwanari/emoticon              |
| Mozc emoticon.tsv    | BSD-3-Clause | https://github.com/google/mozc                    |
| Mozc symbol.tsv      | BSD-3-Clause | https://github.com/google/mozc                    |
