# karukan-im

Karukan の IME 本体。共有エンジンと各プラットフォームのフロントエンドがここにまとまっています。

| ディレクトリ | 説明 |
|---------|------|
| [core/](core/) | 共有IMEエンジン(crate: `karukan-im`) — ステートマシン、ローマ字変換、karukan-imserver(macOS向けJSON-RPCサーバ) |
| [fcitx5/](fcitx5/) | Linux向けフロントエンド(crate: `karukan-fcitx5`) — fcitx5アドオン + C FFI |
| [macos/](macos/) | macOS向けフロントエンド — Swift/InputMethodKit |

キーバインド・設定・辞書などユーザ向けドキュメントは [docs/](../docs/) を、インストール手順は各フロントエンドの README を参照してください。
