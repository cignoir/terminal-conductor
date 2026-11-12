# terminal-conductor 設計書

- 作成日: 2026-11-12
- 状態: M0〜M4 実装済み・動作確認済み（2026-11-12）

## 1. 目的

AI CLI（主に Claude Code）を数十のプロジェクトで並行して使うための、Windows 用の統合ターミナルアプリを作る。

解決したい課題は 2 つある。

| 課題 | 現状 | このアプリでの解決 |
|---|---|---|
| P1. 復元の手間 | 再起動のたびに、Windows Terminal のタブを開き、上下に分割し、`cd` して `claude` を起動し直している | プロジェクト台帳とワークスペース状態を保存する。起動時にタブを復元し、Claude はセッション ID を指定して再開する |
| P2. 何のプロジェクトか分からない | タブ名とパスしか手がかりがない | 説明文、自動要約、git 状態、Claude の最後の返答、最終活動日時をサイドバーと切り替え画面に出す |

付随する価値として、どのプロジェクトの Claude が入力待ちかを一覧で見られるようにする（P3）。

### 対象外（MVP）

- Windows 以外の OS
- 任意のペイン分割（MVP は上下 2 分割固定。データモデルだけ木構造にしておく）
- SSH やリモートセッション
- Claude Code 以外の AI CLI 向けの状態検出（ターミナルとしてはそのまま使える）

## 2. 全体構成

```
┌──────────────────────── Tauri アプリ (terminal-conductor.exe) ───────────────────────┐
│                                                                                     │
│  WebView2 (React + TS)                         Rust (src-tauri)                      │
│  ┌───────────────────────────┐                 ┌───────────────────────────────┐    │
│  │ Sidebar / QuickSwitcher   │  invoke         │ commands (薄い Tauri 層)       │    │
│  │ ProjectView               │ ──────────────> │   │                           │    │
│  │   ├ TerminalPane (top)    │                 │   ▼                           │    │
│  │   └ TerminalPane (bottom) │ <── Channel ─── │ tc-core                       │    │
│  │ xterm.js インスタンス群    │   (出力バイト)   │   ├ PtyManager (ConPTY)        │    │
│  └───────────────────────────┘                 │   ├ ReplayBuffer (ペインごと)  │    │
│                                                │   ├ StatusTracker              │    │
│                                                │   ├ Store (JSON 永続化)        │    │
│                                                │   └ HookServer (named pipe)    │    │
│                                                └───────────────▲───────────────┘    │
└────────────────────────────────────────────────────────────────┼────────────────────┘
                                                                 │ \\.\pipe\terminal-conductor
   ConPTY ── cmd.exe ── claude --session-id ... --settings tc-hooks.json
                              │
                              └─ hook 発火 ── terminal-conductor.exe hook (TC_PANE_ID を付けて送信、即終了)
```

### 技術スタック

| 層 | 採用 | 理由 |
|---|---|---|
| アプリ枠 | Tauri 2 | バイナリを小さく保つ |
| フロントエンド | React + TypeScript + Vite、状態管理は Zustand | |
| ターミナル描画 | `@xterm/xterm` 5.x、`addon-fit`、`addon-webgl`、`addon-unicode11`、`addon-web-links`、`addon-search` | CJK の文字幅は unicode11 で扱う |
| PTY | `portable-pty`（ConPTY） | |
| hook 受信 | Win32 の named pipe サーバー（スレッド） | localhost のポートを開けずに済む。既定の DACL で、書き込めるのは同じユーザーだけ |
| 永続化 | `%APPDATA%\io.github.cignoir.terminal-conductor\*.json`（一時ファイルに書いてからリネームする） | 数十件なら DB は不要 |
| 配布 | NSIS インストーラー（ユーザー単位）＋ GitHub Releases | 自動更新は tauri-plugin-updater。手順は docs/RELEASING.md |

### クレート構成

```
terminal-conductor/
├─ src/                     # React フロントエンド
├─ src-tauri/               # Tauri アプリ本体。commands と配線だけを置く
├─ crates/
│  └─ tc-core/              # pty, hook, status, launch, project, store
│                           # Tauri の型に依存させない（後で別プロセスのホストへ移せるように）
└─ docs/
```

`tc-core` を Tauri から切り離しておくのは、将来「UI を閉じても PTY を生かしておく常駐ホスト」に分離する余地を残すためである。

hook クライアントは別 exe にせず、アプリ本体を `terminal-conductor.exe hook` で呼ぶ。`main()` の先頭で分岐して GUI の初期化をしないので、すぐ終わる。配布物も 1 つで済む。

## 3. データモデル

ペインの ID は `<projectId>:claude` と `<projectId>:shell` で、プロジェクトごとに上下 2 つに固定している。任意の分割に広げるときは、`splitRatio` を木構造のレイアウトに置き換える。

```ts
// projects.json（フロントエンドが書く）: { version: 1, projects: Project[] }
type Project = {
  id: string;              // uuid
  name: string;
  path: string;            // 作業ディレクトリ
  description: string;     // 手書きの一言説明
  summary?: { text: string; generatedAt: number };  // 自動要約（6.2 節）
  color: string;           // サイドバーの色
  tags: string[];
  autoStart: boolean;      // 起動時にすぐ PTY を立ち上げるか（既定 false）
  shell?: "cmd" | "pwsh" | "powershell";  // 未指定なら設定の既定シェル
  continueOnFirstOpen?: boolean;  // 最初に開いたとき claude --continue で直前の会話を引き継ぐ（1 回だけ）
  splitRatio: number;      // Claude ペインの高さの割合
  createdAt: number;
};

// workspace.json（フロントエンドが書く）
type Workspace = {
  openProjectIds: string[];  // サイドバーの「開いているプロジェクト」の並び順
  activeProjectId?: string;
};

// sessions.json（Rust 側が書く）: ペイン ID → SessionInfo
type SessionInfo = {
  sessionId?: string;             // 再開に使う。hook を受けるたびに更新する
  sessionTitle?: string;
  lastAssistantMessage?: string;  // Stop hook の last_assistant_message（600 文字まで）
  lastActivityAt?: number;        // Unix ミリ秒
};

// settings.json（フロントエンドが書く）
type Settings = {
  fontFamily: string;
  fontSize: number;
  defaultShell: "cmd" | "pwsh" | "powershell";
  notifications: boolean;
  scrollback: number;
};
```

ファイルを書き手ごとに分けているのは、hook を受けた Rust 側と、画面を操作したフロントエンド側が同じファイルを同時に書かないようにするため。状態バッジ（作業中など）は保存しない。アプリを起動し直したら、次の hook が来るまで表示しない。

## 4. ターミナル層

### 4.1 PTY

- 1 ペイン = 1 ConPTY = 1 シェル。既定のシェルは `cmd.exe`、プロジェクト単位で変更できる。
- 起動時の環境変数に `TC_PANE_ID=<paneId>` を入れる。hook 側はこれで送信元のペインを判別する。
- 子プロセスはすべて Job Object（`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`）に入れる。アプリが落ちても `claude` や `node` が孤児として残らないようにするため。
- 出力は PTY ごとの読み取りスレッドで受け、読めた単位でそのまま `tauri::ipc::Channel` に生バイトで流す（JSON にしない）。
- ConPTY は起動直後にカーソル位置の問い合わせ（`ESC[6n`）を送り、応答があるまで子プロセスを止める。UI が付く前に起動するペイン（autoStart）でも止まらないよう、Rust 側で最初の 1 回だけ応答し、問い合わせ自体はリプレイ用バッファに残さない。
- アプリ自体が Claude Code の中から起動された場合（開発時など）、`CLAUDECODE` や `CLAUDE_CODE_CHILD_SESSION` などのセッション用の環境変数を引き継ぐと、ペインの Claude が子セッション扱いになり会話が保存されない。PTY を起動するときはこれらを外す。
- 入力は `pty_write(paneId, bytes)`、サイズ変更は `pty_resize(paneId, cols, rows)` で受ける。

### 4.2 画面状態をどこに持つか（決定）

**PTY を持つ Rust 側に、ペインごとの生出力リングバッファ（既定 4MB）を持たせる。** xterm.js のインスタンスは表示用のキャッシュとして扱う。

- WebView をリロードしたり、レンダラーがクラッシュしたりしても、バッファを再生すれば画面を戻せる。
- 見えていないペインの xterm インスタンスは、LRU で最大 N 個（既定 8）まで保持する。それを超えたものは破棄し、次に表示するときにバッファから作り直す。
- WebGL アドオンは表示中のペインにだけ付ける。WebView2 で同時に持てる WebGL コンテキスト数には上限があるとされるため（M0 で実測）。
- 生バイトの再生なので、途中でサイズが変わった分の表示崩れは許容する。Claude の TUI は次の再描画で直る。
- 再生中の出力に含まれる問い合わせ（カーソル位置など）に xterm.js が応答すると、過去の応答がシェルへの入力として紛れ込む。そのため、`pane_attach` が返す再生バイト数を xterm.js が処理し終わるまでは、`onData` を PTY に送らない。

### 4.3 キー操作

アプリのショートカットは最小限にし、それ以外のキーはすべて PTY に渡す。

| キー | 動作 |
|---|---|
| Ctrl+C（選択中のみ）/ Ctrl+Shift+C | コピー。選択がないときの Ctrl+C はそのまま PTY に送る |
| Ctrl+V / Ctrl+Shift+V / 右クリック | 貼り付け（選択中の右クリックはコピー）。ペイン内のプログラムがマウス報告を有効にしているとき（Claude Code など）は、右クリックは xterm.js がプログラムに送り、アプリ側では貼り付けない（両方が貼り付けて 2 回入るため）。Shift+右クリックはマウス報告を通らないので、アプリ側で貼り付け・コピーする |
| Ctrl+Tab / Ctrl+Shift+Tab | 開いているプロジェクトの切り替え |
| Ctrl+1〜9 | n 番目のプロジェクトへ |
| Ctrl+K | クイックスイッチャー（名前、説明、タグ、要約、パスで検索） |
| Alt+↑ / Alt+↓ | 上下ペイン間のフォーカス移動 |
| Shift+Enter | `ESC CR`（Alt+Enter）を送る。Claude Code の入力欄で改行になる |
| Ctrl+, | 設定 |

Shift+Enter で Claude Code 2.1.322 の入力欄が改行されることは確認済み。

## 5. Claude Code 連携

### 5.1 起動と再開（決定）

- 新しく Claude を起動するときは、アプリ側で UUID を生成し `claude --session-id <uuid> --settings <tc-hooks.json>` で起動する。生成した ID は `PaneState.claudeSessionId` に保存する。
- 復元時は `claude --resume <claudeSessionId> --settings <tc-hooks.json>` で起動する。`--continue` はそのディレクトリで最後に使った会話を開くだけなので、同じプロジェクトで別途 `claude` を使っていると別の会話を拾ってしまう。だから復元には使わない。
- **Claude の自動起動は既定でオフ**（特定の AI に依存しないため。2026-11-12 に変更）。オフのときは上のペインも普通のシェルとして開き、前回の会話があれば「再開する」（`--resume`）、フォルダに会話記録があれば「直前の会話を続ける」（`--continue`）のボタンを出す。押すとそのコマンドを打ち込む。設定の `autoStartClaude` をオンにすると、以下のとおり自動で起動・再開する。
- 例外として、自動起動がオンで、プロジェクトを追加するときに「直前の会話を引き継ぐ」を選んだ場合だけ、最初の 1 回を `claude --continue` で起動する（ほかのターミナルからの移行用）。セッション ID は SessionStart の hook で受け取り、以降は `--resume` になる。フォルダから一括で取り込んだプロジェクトでは使わない。ほかのターミナルで同じフォルダの Claude が動いたままだと、同じ会話を 2 か所で使うことになるため、追加のダイアログで先に終了するよう案内する。
- `/clear` すると新しいセッションになる。そのため、hook を受けるたびに payload の `session_id` で `claudeSessionId` を上書きする。

### 5.2 hook の注入（決定）

- グローバルの `~/.claude/settings.json` は編集しない。アプリがデータフォルダに `tc-hooks.json` を生成し、起動のたびに `--settings` で渡す。
- 各ペインのシェルに `claude` エイリアスを定義し、`--settings` を自動で付ける。cmd.exe は `cmd /k init.cmd` で doskey マクロを読み込み、PowerShell は `-NoExit -Command` で関数を定義する。アプリが Claude を起動するときも、このエイリアスを打ち込む形にしている。こうすると、下のペインで手で `claude` と打った場合にも hook が効く。
  - 当初案の `claude.cmd` シムはやめた。バッチファイル経由だと、Ctrl+C のたびに「バッチ ジョブを終了しますか」が出るため。
- hook クライアント（`terminal-conductor.exe hook`）は次の順に動く。
  1. `TC_PANE_ID` が無ければ、何もせず exit 0
  2. stdin の JSON に `TC_PANE_ID` を付けて named pipe に送る（タイムアウトは 200ms）
  3. pipe に繋がらなくても exit 0。アプリが落ちていても Claude の動作は止めない

登録する hook は次のとおり。

| イベント | 用途 |
|---|---|
| `SessionStart` | セッション ID の取得。`source` が startup / resume / clear / compact のどれかを見る |
| `UserPromptSubmit` | working へ遷移 |
| `PreToolUse` / `PostToolUse` | working の維持（許可プロンプトが解決したことの検出） |
| `Notification`（`permission_prompt`） | needs_permission へ遷移 |
| `Notification`（`idle_prompt`） | 約 60 秒放置された done を目立たせる |
| `Stop` | done へ遷移し、`last_assistant_message` を保存 |
| `StopFailure` | error へ遷移 |
| `SessionEnd` | exited へ遷移 |

### 5.3 状態遷移（ペイン ID 単位）

```
               PTY 起動前
 not_started ─────────────┐
      │ PTY 起動           │
      ▼                   │
    idle ◄────────────────┼──────────────────────────┐
      │ UserPromptSubmit  │                          │ ユーザーがそのペインを表示
      ▼                   │                          │
   working ── Stop ──> done_unread ──────────────────┘
      │  ▲
      │  │ PreToolUse / PostToolUse
      ▼  │
 needs_permission    (Notification: permission_prompt)

 任意の状態 ── StopFailure ──> error
 任意の状態 ── SessionEnd または PTY 終了 ──> exited
```

- PTY の生死を最優先にする。プロセスが終了したら、どの状態からでも exited にする。Claude が落ちたのに working のまま残ることを防ぐため。
- サブエージェントの hook（payload に `agent_id` がある）は、メインのセッションの状態を変えない。
- 許可を出したあと、そのツールが終わる（PostToolUse）までは hook が来ない。長いコマンドだと「許可待ち」のまま残ってしまうので、needs_permission のペインにユーザーが何か入力したら、その時点で working にする（フロントエンド側の処理）。
- 「未読」はフロントエンドが持つ。表示していないプロジェクト（またはウィンドウが非アクティブなとき）が done、needs_permission、error になったら未読にし、そのプロジェクトを表示したら消す。
- サイドバーには優先度順にバッジを出す（needs_permission > error > done > working > idle > exited）。
- 未読になったとき、Windows のトースト通知を出し、ウィンドウが非アクティブならタスクバーを点滅させる（設定で無効にできる）。

### 5.4 将来の候補

Claude Code は、OSC 7501 Program Status Protocol に対応したターミナルに対してセッション状態（作業中、入力待ち、完了）を送る。このアプリはターミナル自体を持っているので、仕様が入手できればこれを実装し、hook を使わない状態検出にも切り替えられる。MVP は hook を主経路にする。

### 5.5 シェルへのパスの渡し方

`claude` エイリアスに使う Claude の実行ファイルと `tc-hooks.json` のパスは、環境変数（`TC_CLAUDE_EXE`、`TC_HOOKS_FILE`）で渡す。cmd.exe はバッチファイルを OEM コードページで読むので、ユーザー名に日本語が含まれると、ファイルに書いたパスが化けるため。`init.cmd` には ASCII の `@doskey claude="%TC_CLAUDE_EXE%" --settings "%TC_HOOKS_FILE%" $*` だけを書く。

hook のコマンドは、Claude Code が bash と PowerShell のどちらで実行しても動くように、アプリのパスに空白があれば 8.3 形式の短いパスに置き換える（短いパスがないボリュームでは引用符で囲む）。空白と日本語を含むフォルダから起動して、状態表示が動くことを確認した（2026-11-12）。

## 6. プロジェクトの識別（P2）

### 6.1 一覧に出す情報

サイドバーの各行と、ホバー時のカードに次を出す。

- 名前、色、状態バッジ
- 手書きの説明（1 行）
- Claude の最後の返答の先頭（`last_assistant_message` から）
- セッションタイトル（hook の `session_title`）
- git のブランチ名と未コミット変更の有無（タブを開いたとき、およびフォーカスが戻ったときに `git status --porcelain -b` を実行）
- 最終活動日時（「3 日前」のような相対表記）

### 6.2 自動要約

- 情報パネルの「生成」「更新」ボタンを押したときだけ生成する（使用量を勝手に消費しないため）。
- アプリが README.md、CLAUDE.md、docs/design.md、package.json、Cargo.toml、`git log --oneline -20`、ファイル一覧を読み、先頭から一定量だけをプロンプトに入れる。
- `claude -p --tools "" --max-turns 1 --output-format text --no-session-persistence` を、`TC_PANE_ID` を付けず、`--settings` も渡さずに実行する。こうすると、こちらの hook は何もしない。
- 結果を `Project.summary` に保存する。

### 6.3 プロジェクトの追加

- フォルダを選ぶか、パスを貼り付けて追加する（ドラッグ＆ドロップは未実装）。
- 指定したフォルダの直下を走査し、`.git`、package.json、Cargo.toml、pyproject.toml、CLAUDE.md のどれかがあるフォルダを候補として一括で取り込めるようにする（初回の移行を楽にするため）。

## 7. 起動と復元

- **既定は遅延起動**。前回開いていたタブはすべて並べるが、PTY は最初にそのタブを表示したときに起動する。Claude（Node プロセス）を数十個同時に起こすと、メモリを数 GB 使うため。
- `autoStart: true` のプロジェクトだけ、起動時にすぐ PTY を立ち上げる。
- 復元されるもの: タブの並び、分割比率、Claude の会話（`--resume` による）、状態バッジ、最後の返答。
- 復元されないもの: シェルペインのスクロールバック、実行中だったコマンド。
- 二重起動は `tauri-plugin-single-instance` で防ぐ。
- 終了するとき（ウィンドウを閉じる、プロジェクトを閉じる、新しい会話で起動し直す）は、各ペインに Ctrl+C を 2 回送って Claude を正常に終わらせ、約 1.4 秒待ってから残ったプロセスを止める。いきなり強制終了すると、Claude が「前回の起動に失敗した」と判断し、次の `--resume` で表示モードを落とすため。
- OS の再起動やログオフでアプリが閉じるイベントを受けずに止められた場合は、この手順を通らない。その場合も子プロセスは Job Object で一緒に終了し、次に起動したとき `--resume` で会話に戻ることを確認した（8 節）。

## 8. リスクと M0 で確かめること

**最大のリスクは日本語 IME** である。xterm.js は隠し textarea で IME を受けるため、変換候補ウィンドウの位置や確定の挙動に問題が出ることがある。Claude への入力はほぼ日本語なので、ここが実用に耐えなければ B 案は成り立たない。

| # | 確認項目 | 合格基準 |
|---|---|---|
| 1 | 日本語 IME | 変換中の文字列がカーソル位置に出る。候補ウィンドウがカーソル付近に出る。確定や再変換で文字化けや二重入力が起きない。Claude の入力欄で長文を打てる |
| 2 | CJK の文字幅 | 全角文字や罫線で Claude の TUI の枠がずれない |
| 3 | Claude の TUI 再描画 | ストリーミング出力、スピナー、許可ダイアログ、リサイズ時に描画が崩れない |
| 4 | 貼り付け | 複数行の貼り付けが bracketed paste として届く |
| 5 | スループット | 大量出力（`type` で巨大ファイルを流すなど）で UI が固まらない |
| 6 | WebGL の上限 | WebView2 で WebGL コンテキストを同時にいくつ持てるか |
| 7 | ConPTY | OS 標準の ConPTY で問題がないか。問題があれば、新しい `conpty.dll` と `OpenConsole.exe` の同梱を検討する |
| 8 | `--settings` の hook | ユーザー設定の hook と併用したとき、両方とも動くか（ドキュメントに明記がないため実測する） |
| 9 | session_id | compact の前後で `session_id` が変わるか。`--resume <id>` で同じ ID が続くか |

### M0 の結果（2026-11-12）

| # | 結果 |
|---|---|
| 1 | CDP の `Input.imeSetComposition` / `Input.insertText` で変換を再現して確認。変換中の文字列は Claude の入力欄のカーソル位置に出て、確定すると 1 回だけ入力される（二重入力なし）。IME が候補ウィンドウの位置に使う隠し textarea は、カーソルに合わせて移動する。物理キーボードと OS の IME で候補ウィンドウの位置を目で見る確認は、使い始めに行う |
| 2 | 日本語の表示に崩れなし（unicode11 アドオン使用） |
| 3 | Claude Code 2.1.322 の起動画面、信頼ダイアログ、応答、入力欄が正しく描画される |
| 4 | シェルペインで Ctrl+V と右クリックの貼り付けを確認（1 行）。複数行は xterm.js の `paste()` に任せている |
| 5 | 12 万行（約 8.8MB）を cmd の `type` で流して約 4.9 秒。その間の画面のフレーム遅延は最大 20ms で、操作は止まらなかった |
| 6 | 5 プロジェクト（10 ペイン）を開いて切り替えても問題なし。表示中の 2 ペインにだけ WebGL を付けている |
| 7 | OS 標準の ConPTY で動作（起動時の問い合わせへの応答が必要だった。4.1 節） |
| 8 | 併用できる。ユーザー設定の PreToolUse hook（Bash を検査するもの）がコマンドを止め、同時にこちらの hook で状態が 作業中 → 完了 と変わった |
| 9 | `--session-id` で起動し、アプリ再起動後に `--resume <id>` で同じ会話に戻れた |

### M0 以降に確認したこと

- 裏のプロジェクトで Claude が完了すると、サイドバーの行が未読になり、そのプロジェクトを表示すると消える。
- 5 プロジェクトを開くと、一番古いプロジェクトの xterm インスタンスがキャッシュから破棄される。開き直すと、Rust 側のバッファから画面が作り直される。
- ウィンドウを閉じて起動し直しても、Claude は通常の表示モードで会話に戻る。

- 許可待ち: 許可を求めるモードの Claude にファイルを作らせると、バッジが 作業中 → 許可待ち になり、許可すると 作業中 → 完了 に変わった。下のペインで手で打った `claude` にも hook が効いた。
- 強制終了（OS の再起動やログオフで止められた場合に相当）: claude.exe や cmd.exe は Job Object によって一緒に終了し、残らなかった。起動し直すと、プロジェクトと Claude の会話が `--resume` で戻った。
- インストーラー: `/S` でユーザー単位にインストールし、インストールした exe で起動、会話の再開、hook の受信まで動いた。hook のコマンドはインストール先の exe に書き換わる。アンインストールも正常に終わった。

### 既知の制限

- リプレイ用バッファ（4MB）が先頭から切り捨てられたあとに作り直すと、それより前に送られた端末のモード設定（代替画面など）が失われ、表示が崩れることがある。
- cmd.exe の既定のコードページ（932）のままなので、UTF-8 のファイルを `type` すると文字化けする。Windows Terminal で cmd を使った場合と同じ挙動。
- 状態バッジは保存しないので、アプリを起動し直した直後は次の hook が来るまで表示されない。

1〜3 のどれかが解決できない場合は A 案（Windows Terminal を外部から制御する）に切り替える。`tc-core` の Store、StatusTracker、HookServer は A 案でもそのまま使える。

## 9. マイルストーン

M0〜M4 はすべて実装し、8 節のとおり動作を確認した（2026-11-12）。

| # | 内容 | 完了条件 |
|---|---|---|
| M0 | スパイク: Tauri + xterm.js + portable-pty で 1 ペインを作り、`claude` を動かす | 8 節の 1〜7 の結果を記録し、続行か中止かを判断する |
| M1 | コア: プロジェクト台帳、サイドバー、上下分割、タブ切り替え、永続化、遅延復元、`--session-id` と `--resume` | 再起動後、すべてのタブと Claude の会話が戻る |
| M2 | 状態: hook クライアント（`<exe> hook`）、HookServer、状態遷移、バッジ、トースト通知、シェルの `claude` エイリアス | 入力待ちのプロジェクトが一目で分かる |
| M3 | 識別: クイックスイッチャー、git 情報、最後の返答、自動要約、フォルダの一括取り込み | 「これ何だっけ」が起きない |
| M4 | 仕上げ: 設定画面、テーマ、フォント設定、インストーラー | 日常使いに移行できる |

## 10. 公開にあたっての設定

- CSP を有効にしている（`default-src 'self'`。xterm.js がインラインスタイルを使うため `style-src` だけ `'unsafe-inline'` を許可）。
- 自動更新は GitHub Releases の `latest.json` を見る。更新ファイルは鍵で署名し、アプリは埋め込みの公開鍵で確かめる。手順は docs/RELEASING.md。
- 入力をログに出す `TC_DEBUG_INPUT` は開発版だけで有効。
- 画面に与える権限から `updater:default` を外している。理由は docs/RELEASING.md。
- ライセンスは MIT。README・コードコメント・UI は英語を基本にし、UI は `src/i18n.ts` で英語と日本語を切り替える（既定は Windows の表示言語）。英語が元の文言で、ほかの言語は全キーをそろえないと型チェックが通らない。要約も UI の言語で書かせる。

## 11. 会話の検索

「どのフォルダで AI と話したか分からない」を解くため、Claude Code・Codex CLI・Antigravity CLI の会話をまたいで検索する（2026-11-12 追加）。

- 記録の場所と形式：Claude は `~/.claude/projects/<フォルダ>/<セッション ID>.jsonl`（各行に `cwd`、タイトルは `aiTitle`）。サブフォルダの `subagents/` は除く。Codex は `~/.codex/sessions/年/月/日/rollout-*.jsonl.zst`（zstd 圧縮。1 行目の `session_meta` に `id` と `cwd`、本文は `response_item` の message。サブエージェントのスレッドは除く。タイトルは `session_index.jsonl` の `thread_name`）。agy は `conversation_summaries.db`（SQLite。タイトル・プレビュー・`workspace_uris`）で、本文は protobuf のため対象外。
- 記録は Claude が約 7.4GB、Codex が圧縮後で約 4.4GB あり、毎回読むことはできない。発言のテキストだけ（1 会話 1MB まで）を SQLite の索引に入れ、ファイルのサイズと更新時刻で差分だけ読み直す。この PC での実測は、初回 17 秒（195 会話）、2 回目以降 1 秒未満、検索 10〜25ms、索引 5〜8MB。
- 検索は空白区切りの語をすべて含む会話（本文・タイトル・フォルダのどれか）を新しい順に出す。日本語にも効くよう、トークナイザーを使わない部分一致（LIKE）にしている。
- セキュリティ：既定でオフにして初回に同意を取る。索引はローミングしない `%LOCALAPPDATA%` に置く。`secure_delete` を有効にする。設定から削除できる。シェルに打ち込む会話 ID は英数字・`-`・`_` だけを許可する。すでに動いているペインには確認してから打ち込む。Claude の自動起動がオンの場合、Claude の会話はペインの会話として登録してから開き（自動起動がそれを再開する）、ほかの CLI の会話は確認を出す。

## 12. Codex CLI と Antigravity CLI の状態表示

- 両方とも、hooks はあるが Claude のようには使えない。Codex は `-c` で渡した hook をユーザーが一度信頼しないと動かさない（信頼を飛ばすオプションはリポジトリの hook まで無確認で動かすので使わない）。agy はグローバルの設定ファイルにしか hook を書けない。
- そのかわり両方とも **OSC 7501**（プログラムの状態を知らせるエスケープシーケンス）を送る。実物の書式（2026-11-12 に Codex CLI 0.167.2 と Antigravity CLI 1.5.6 で記録）：`ESC ] 7501 ; state=<idle|working|blocked|done|error>[:kind=<permission|auth|...>]:app=<codex|antigravity>[:msg=<Base64 の UTF-8>] BEL`。agy は `done` の msg に最後の返答、`blocked` の msg に確認の文言を入れる。
- 読み取りは Rust 側の PTY 読み取りループで行う（`tc-core/src/osc.rs`）。xterm.js 側で読むと、キャッシュから外れて破棄された裏のペインの状態を取りこぼすため。読み取りの間で途切れた列もつなぐ。
- 状態の対応：working → 作業中、blocked → 許可待ち、done → 完了、error → エラー、idle → 待機中。ただし done / error の直後に来る idle では表示を戻さない（次のターンが始まるまで結果を見せる）。
- agy は対応ターミナルを自分で判定するので、ペインに `AGY_CLI_OSC7501=on` を設定する。
- 残る制限：CLI を終了しても終了の通知は来ないので、最後の状態が残る。

## 13. その他の UI

- WebView の既定の右クリックメニュー（戻る、再読み込み、検証など）はアプリ全体で出さない。サイドバーのプロジェクトには専用のメニュー（エクスプローラーで開く・開く・編集・閉じる）を出す。ターミナルの右クリック（コピー／貼り付け）はそのまま。
- プロジェクトのヘッダーに「エクスプローラー」ボタンを置く。
- ツールバーのボタン（サイドバー、プロジェクトのヘッダー、ペインのバー、会話検索の結果）はアイコン（lucide）だけにし、説明とショートカットはツールチップで出す。ツールチップはウィンドウに1つだけで、どの要素の上でも切れないように最上位に描く。ダイアログの確定・取り消しと最初の画面の大きなボタンは、押した結果が分かるようにテキストのままにする。
- ダウンロードした更新は、既定でアプリを閉じるときにインストールする（起動し直さない）。作業の途中で再起動しなくて済むようにするため。
- hook の named pipe の名前に、実行ファイルの正規化したパスのハッシュを入れる。開発版とインストール版を同時に動かしても、hook の通知を取り合わない。

## 14. 未決事項

- スクロールバックの行数と、リングバッファのサイズの既定値（M0 のメモリ実測の後で決める）
- 生出力のファイル保存（ペインのログを残すか）
- 自動要約の再生成タイミング（手動のみか、一定期間ごとか）
