// UI strings. English is the source of truth; every other language must
// provide every key (enforced by the type of `ja`).
//
// Placeholders are written as {name} and filled from the `vars` argument.

import { useStore } from "./store";

const en = {
  // App / empty state
  "app.loading": "Loading…",
  "app.claudeMissing": "The claude command was not found on PATH. The top pane will run as a plain shell.",
  "empty.welcome": "Welcome",
  "empty.pick": "Pick a project",
  "empty.welcomeBody": "Add a project folder to open Claude Code on top and a shell below.",
  "empty.pickBody": "Choose one from the list on the left, or press Ctrl+K to search.",
  "empty.add": "Add project",
  "empty.import": "Import from folder",
  "keys.search": "Search and open a project",
  "keys.cycle": "Cycle through open projects",
  "keys.nth": "Go to the nth project",
  "keys.panes": "Move between the Claude and shell panes",
  "keys.newline": "New line in Claude's prompt",
  "keys.history": "Search past AI conversations",
  "keys.images": "Show or hide images and videos",

  // Sidebar
  "sidebar.settings": "Settings (Ctrl+,)",
  "sidebar.settingsUpdate": "Settings (Ctrl+,) — an update is available",
  "sidebar.filter": "Filter",
  "sidebar.open": "Open projects",
  "sidebar.others": "Other projects",
  "sidebar.empty": "No projects yet. Add one with the buttons below.",
  "sidebar.add": "Add",
  "sidebar.import": "Import from folder",
  "sidebar.history": "Conversations",
  "sidebar.moveUp": "Move up",
  "sidebar.moveDown": "Move down",
  "sidebar.close": "Close (stops its processes)",
  "sidebar.confirmClose":
    "Close {name}?\nIts processes will be stopped. The Claude conversation resumes the next time you open it.",

  // Status
  "status.idle": "Idle",
  "status.working": "Working",
  "status.needs_permission": "Needs permission",
  "status.done": "Done",
  "status.error": "Error",
  "status.exited": "Exited",
  "status.notStarted": "Not started",

  // Project header
  "project.gitBranch": "git branch",
  "project.openFolder": "Open in Explorer",
  "menu.explorer": "Explorer",
  "menu.open": "Open",
  "menu.close": "Close",
  "project.info": "Info",
  "project.hideInfo": "Hide info",
  "project.newConversation": "New conversation",
  "project.newConversationTip": "Restart Claude with a new conversation",
  "project.summaryUpdateTip": "Regenerate the summary",
  "project.summaryGenerateTip": "Generate a summary",
  "pane.resumeTip": "Resume the previous conversation",
  "pane.continueTip": "Continue this folder's latest conversation",
  "pane.newClaudeTip": "Start Claude with a new conversation",
  "project.edit": "Edit",

  // Image panel
  "images.title": "Images & videos",
  "images.show": "Show images and videos",
  "images.hide": "Hide images and videos",
  "images.toggleNew": "Show images and videos ({n} new)",
  "images.empty": "Image and video files that Claude mentions in its replies in the top pane show up here.",
  "images.clear": "Clear the list",
  "images.confirmClear": "Clear the list? The files themselves are not deleted.",
  "images.close": "Close the panel",
  "images.resize": "Drag to resize",
  "images.missing": "File not found",
  "images.open": "Open in the default app",
  "images.reveal": "Show in Explorer",
  "images.copyPath": "Copy path",
  "images.copyImage": "Copy image",
  "images.copied": "Copied the image",
  "images.copyFailed": "Couldn't copy the image: {error}",
  "images.remove": "Remove from the list",
  "images.prev": "Previous",
  "images.next": "Next",
  "images.fit": "Fit",
  "images.actual": "100%",
  "images.closeViewer": "Close",
  "project.confirmNewConversation":
    "Close the current conversation and start Claude with a new one?\nYou can reopen the previous one with claude --resume.",
  "project.summary": "Summary",
  "project.summaryGenerate": "Generate",
  "project.summaryUpdate": "Update",
  "project.summaryGenerating": "Generating…",
  "project.summaryGenerated": "(generated {when})",
  "project.summaryEmpty": "Claude writes a summary from the README, git log and similar files.",
  "project.lastReply": "Claude's last reply",
  "project.noReply": "None yet",
  "project.resizeHandle": "Drag to resize (double-click to reset)",

  // Pane
  "pane.exited": "The process has exited",
  "pane.restart": "Restart",
  "pane.restartFresh": "Start a new conversation",
  "pane.resumeFound": "This pane has a previous Claude conversation.",
  "pane.continueFound": "Claude has conversations for this folder.",
  "pane.resume": "Resume",
  "pane.continue": "Continue latest",
  "pane.newClaude": "New conversation",
  "pane.dismiss": "Dismiss",

  // Quick switcher
  "switcher.placeholder": "Search projects (name, description, tags, path)",
  "switcher.none": "No matches",

  // Project dialog
  "dialog.addTitle": "Add project",
  "dialog.editTitle": "Edit project",
  "dialog.folder": "Folder",
  "dialog.browse": "Browse",
  "dialog.name": "Name",
  "dialog.description": "Description",
  "dialog.descriptionHint": "A one-line note shown in the list. Say what the project is so you never have to wonder.",
  "dialog.descriptionPlaceholder": "e.g. Internal inventory API, shipping next month",
  "dialog.tags": "Tags",
  "dialog.tagsHint": "Comma-separated. Used for search.",
  "dialog.tagsPlaceholder": "work, rust",
  "dialog.color": "Color",
  "dialog.shell": "Shell",
  "dialog.autoStart": "Start as soon as the app starts (otherwise on first open)",
  "dialog.continue": "Continue this folder's latest Claude conversation (claude --continue)",
  "dialog.continueHint":
    "If Claude is running in this folder in another terminal, exit it first. Otherwise the same conversation would be used in two places.",
  "dialog.remove": "Remove from list",
  "dialog.confirmRemove": "Remove {name} from the list?\nThe folder itself is not deleted.",
  "dialog.cancel": "Cancel",
  "dialog.save": "Save",
  "dialog.addAndOpen": "Add and open",
  "dialog.folderNotFound": "Folder not found",
  "dialog.alreadyAdded": "This folder has already been added",

  // Shells
  "shell.default": "Default (from settings)",
  "shell.cmd": "Command Prompt (cmd.exe)",
  "shell.pwsh": "PowerShell 7 (pwsh.exe)",
  "shell.powershell": "Windows PowerShell (powershell.exe)",

  // Import dialog
  "import.title": "Import from folder",
  "import.help":
    "If the chosen folder is a project (it has .git, package.json, Cargo.toml, CLAUDE.md or similar), only that folder is added. Otherwise its subfolders that are projects are listed.",
  "import.parent": "Parent folder",
  "import.searching": "Searching…",
  "import.none": "Nothing found",
  "import.already": "Already added",
  "import.add": "Add {count}",

  // Settings dialog
  "settings.title": "Settings",
  "settings.language": "Language",
  "settings.languageAuto": "Automatic (Windows display language)",
  "settings.font": "Font",
  "settings.fontHint": "A CSS font-family. Add a CJK font at the end for clean full-width characters.",
  "settings.fontSize": "Font size",
  "settings.scrollback": "Scrollback lines",
  "settings.defaultShell": "Default shell",
  "settings.notifications": "Show a Windows notification when a background project needs permission or finishes",
  "settings.autoOpenImages": "Open the image and video panel when Claude mentions a new one",
  "settings.autoStartClaude": "Start Claude Code automatically in the top pane",
  "settings.autoStartClaudeHint":
    "Off: the top pane opens as a plain shell and offers to resume the previous conversation. On: Claude starts and resumes by itself.",
  "settings.historySearch": "Conversation search (Ctrl+Shift+F)",
  "settings.historySearchHint":
    "Keeps a local index of your past AI conversations so you can find which folder they happened in.",
  "settings.historyDelete": "Delete index",
  "settings.historyDeleteConfirm": "Delete the conversation search index? Your original conversation files are not touched.",
  "settings.historyDeleted": "Deleted.",
  "settings.claudeMissing": "Not found (check PATH)",
  "settings.dataDir": "Data folder",

  // Update panel
  "update.title": "Updates",
  "update.current": "Current v{version}",
  "update.dev": "Not checked in development builds",
  "update.checking": "Checking…",
  "update.upToDate": "Up to date",
  "update.available": "v{version} is available",
  "update.downloading": "Downloading… {percent}%",
  "update.ready": "v{version} is ready",
  "update.installing": "Stopping Claude and updating…",
  "update.error": "Could not check for updates",
  "update.check": "Check",
  "update.download": "Download",
  "update.install": "Restart and update",
  "update.installTitle": "Lets each Claude exit cleanly, then updates and restarts",
  "update.readyHint": "On restart each pane's Claude exits cleanly and resumes the same conversation after the update.",
  "update.readyOnExit":
    "v{version} will be installed the next time you close the app, so nothing is interrupted. Or restart and update now.",
  "update.installOnExit": "Install downloaded updates when the app closes (without restarting it)",
  "update.notes": "What's new",

  // Conversation search
  "history.title": "Search conversations",
  "history.placeholder": "Search past conversations (words, file names, errors, folders)",
  "history.indexing": "Indexing… {done}/{total}",
  "history.indexed": "{count} conversations",
  "history.copyCommand": "Copy command",
  "history.copied": "Copied: {command}",
  "history.folderMissing": "The folder no longer exists: {path}",
  "history.paneNotReady": "The pane did not start in time. Try again.",
  "history.confirmType":
    "The top pane is already running. Type this command into it?\n\n{command}\n\nMake sure it is at a shell prompt. Choose No to copy the command instead.",
  "history.consentIntro":
    "Search your past conversations with Claude Code, Codex CLI and Antigravity CLI, and reopen them in the right folder.",
  "history.consentSources":
    "Terminal Conductor reads the conversation files these tools keep on this PC (~/.claude, ~/.codex, ~/.gemini/antigravity-cli).",
  "history.consentCopy":
    "To search quickly, it keeps a copy of the conversation text (your messages and the AI's replies, not tool output) in an index file. Anything you pasted into a conversation, such as keys or passwords, ends up in that copy too.",
  "history.consentLocal":
    "The index stays in the local, non-roaming app data folder and is never sent anywhere.",
  "history.consentDelete": "You can turn this off and delete the index at any time in Settings.",
  "history.enable": "Turn on and build the index",

  // Notifications
  "notify.permission": "Waiting for permission",
  "notify.error": "Stopped with an error",
  "notify.done": "Done",

  // Relative time
  "time.now": "just now",
  "time.minutes": "{n}m ago",
  "time.hours": "{n}h ago",
  "time.days": "{n}d ago",
  "time.months": "{n}mo ago",
  "time.years": "{n}y ago",
} as const;

export type MessageKey = keyof typeof en;

const ja: Record<MessageKey, string> = {
  "app.loading": "読み込み中…",
  "app.claudeMissing": "claude コマンドが PATH に見つかりません。上のペインは普通のシェルとして動きます。",
  "empty.welcome": "ようこそ",
  "empty.pick": "プロジェクトを選んでください",
  "empty.welcomeBody": "プロジェクトのフォルダを追加すると、上に Claude Code、下にシェルを並べて開きます。",
  "empty.pickBody": "左の一覧から選ぶか、Ctrl+K で検索して開きます。",
  "empty.add": "プロジェクトを追加",
  "empty.import": "フォルダから取り込む",
  "keys.search": "プロジェクトを検索して開く",
  "keys.cycle": "開いているプロジェクトを順に切り替え",
  "keys.nth": "n 番目のプロジェクトへ",
  "keys.panes": "Claude とシェルのペインを移動",
  "keys.newline": "Claude の入力欄で改行",
  "keys.history": "過去の AI との会話を検索",
  "keys.images": "画像・動画パネルの表示・非表示",

  "sidebar.settings": "設定 (Ctrl+,)",
  "sidebar.settingsUpdate": "設定 (Ctrl+,) — 更新があります",
  "sidebar.filter": "絞り込み",
  "sidebar.open": "開いているプロジェクト",
  "sidebar.others": "ほかのプロジェクト",
  "sidebar.empty": "まだプロジェクトがありません。下のボタンから追加してください。",
  "sidebar.add": "追加",
  "sidebar.import": "フォルダから取り込む",
  "sidebar.history": "会話を検索",
  "sidebar.moveUp": "上へ",
  "sidebar.moveDown": "下へ",
  "sidebar.close": "閉じる（プロセスを終了）",
  "sidebar.confirmClose":
    "{name} を閉じますか？\n実行中のプロセスは終了します。Claude の会話は次に開いたときに再開できます。",

  "status.idle": "待機中",
  "status.working": "作業中",
  "status.needs_permission": "許可待ち",
  "status.done": "完了",
  "status.error": "エラー",
  "status.exited": "終了",
  "status.notStarted": "未起動",

  "project.gitBranch": "git ブランチ",
  "project.openFolder": "エクスプローラーで開く",
  "menu.explorer": "エクスプローラー",
  "menu.open": "開く",
  "menu.close": "閉じる",
  "project.info": "情報",
  "project.hideInfo": "情報を閉じる",
  "project.newConversation": "新しい会話",
  "project.newConversationTip": "新しい会話で Claude を起動し直す",
  "project.summaryUpdateTip": "要約を作り直す",
  "project.summaryGenerateTip": "要約を作る",
  "pane.resumeTip": "前回の会話を再開する",
  "pane.continueTip": "このフォルダの直前の会話を続ける",
  "pane.newClaudeTip": "新しい会話で Claude を起動する",
  "project.edit": "編集",

  // 画像パネル
  "images.title": "画像・動画",
  "images.show": "画像・動画を表示",
  "images.hide": "画像・動画を隠す",
  "images.toggleNew": "画像・動画を表示（新しいもの {n} 件）",
  "images.empty": "上のペインの Claude が返答の中で触れた画像や動画のファイルが、ここに並びます。",
  "images.clear": "一覧を空にする",
  "images.confirmClear": "一覧を空にしますか？ファイル自体は削除しません。",
  "images.close": "パネルを閉じる",
  "images.resize": "ドラッグで幅を変更",
  "images.missing": "ファイルが見つかりません",
  "images.open": "既定のアプリで開く",
  "images.reveal": "エクスプローラーで表示",
  "images.copyPath": "パスをコピー",
  "images.copyImage": "画像をコピー",
  "images.copied": "画像をコピーしました",
  "images.copyFailed": "画像をコピーできませんでした: {error}",
  "images.remove": "一覧から外す",
  "images.prev": "前へ",
  "images.next": "次へ",
  "images.fit": "全体表示",
  "images.actual": "100%",
  "images.closeViewer": "閉じる",
  "project.confirmNewConversation":
    "今の会話を閉じて、新しい会話で Claude を起動し直しますか？\n前の会話は claude --resume で開けます。",
  "project.summary": "概要",
  "project.summaryGenerate": "生成",
  "project.summaryUpdate": "更新",
  "project.summaryGenerating": "生成中…",
  "project.summaryGenerated": "（生成: {when}）",
  "project.summaryEmpty": "README や git log から Claude が要約を作ります。",
  "project.lastReply": "Claude の最後の返答",
  "project.noReply": "まだありません",
  "project.resizeHandle": "ドラッグで高さを変更（ダブルクリックで元に戻す）",

  "pane.exited": "プロセスは終了しました",
  "pane.restart": "再起動",
  "pane.restartFresh": "新しい会話で起動",
  "pane.resumeFound": "このペインには前回の Claude の会話があります。",
  "pane.continueFound": "このフォルダには Claude の会話があります。",
  "pane.resume": "再開する",
  "pane.continue": "直前の会話を続ける",
  "pane.newClaude": "新しい会話",
  "pane.dismiss": "閉じる",

  "switcher.placeholder": "プロジェクトを検索（名前・説明・タグ・パス）",
  "switcher.none": "見つかりません",

  "dialog.addTitle": "プロジェクトを追加",
  "dialog.editTitle": "プロジェクトを編集",
  "dialog.folder": "フォルダ",
  "dialog.browse": "選ぶ",
  "dialog.name": "名前",
  "dialog.description": "説明",
  "dialog.descriptionHint": "一覧に出る一言メモ。何のプロジェクトかを書いておくと迷いません。",
  "dialog.descriptionPlaceholder": "例: 社内向け在庫管理 API。来月リリース予定",
  "dialog.tags": "タグ",
  "dialog.tagsHint": "カンマ区切り。検索に使えます。",
  "dialog.tagsPlaceholder": "仕事, rust",
  "dialog.color": "色",
  "dialog.shell": "シェル",
  "dialog.autoStart": "アプリ起動時にすぐ立ち上げる（オフなら最初に開いたときに起動）",
  "dialog.continue": "このフォルダの直前の Claude の会話を引き継ぐ（claude --continue）",
  "dialog.continueHint":
    "ほかのターミナルでこのフォルダの Claude を動かしている場合は、先に終了してください。動かしたままだと、同じ会話を 2 か所で使うことになります。",
  "dialog.remove": "一覧から削除",
  "dialog.confirmRemove": "{name} を一覧から削除しますか？\nフォルダ自体は消えません。",
  "dialog.cancel": "キャンセル",
  "dialog.save": "保存",
  "dialog.addAndOpen": "追加して開く",
  "dialog.folderNotFound": "フォルダが見つかりません",
  "dialog.alreadyAdded": "このフォルダは登録済みです",

  "shell.default": "既定（設定に従う）",
  "shell.cmd": "コマンドプロンプト (cmd.exe)",
  "shell.pwsh": "PowerShell 7 (pwsh.exe)",
  "shell.powershell": "Windows PowerShell (powershell.exe)",

  "import.title": "フォルダから取り込む",
  "import.help":
    "選んだフォルダ自体がプロジェクト（.git・package.json・Cargo.toml・CLAUDE.md などがある）なら、そのフォルダだけを追加します。そうでなければ、直下にあるプロジェクトのフォルダを一覧にします。",
  "import.parent": "親フォルダ",
  "import.searching": "探しています…",
  "import.none": "見つかりませんでした",
  "import.already": "登録済み",
  "import.add": "{count} 件を追加",

  "settings.title": "設定",
  "settings.language": "言語",
  "settings.languageAuto": "自動（Windows の表示言語）",
  "settings.font": "フォント",
  "settings.fontHint": "CSS の font-family。日本語フォントを後ろに足すと全角がきれいに出ます。",
  "settings.fontSize": "フォントサイズ",
  "settings.scrollback": "スクロールバック行数",
  "settings.defaultShell": "既定のシェル",
  "settings.notifications": "裏のプロジェクトが許可待ち・完了になったら Windows の通知を出す",
  "settings.autoOpenImages": "Claude が新しい画像や動画に触れたらパネルを開く",
  "settings.autoStartClaude": "上のペインで Claude Code を自動で起動する",
  "settings.autoStartClaudeHint":
    "オフ: 上のペインは普通のシェルとして開き、前回の会話を再開するボタンを出します。オン: Claude が自動で起動し、前回の会話を再開します。",
  "settings.historySearch": "会話の検索（Ctrl+Shift+F）",
  "settings.historySearchHint": "過去の AI との会話の索引をこの PC に作り、どのフォルダで話したかを探せるようにします。",
  "settings.historyDelete": "索引を削除",
  "settings.historyDeleteConfirm": "会話検索の索引を削除しますか？元の会話の記録には触れません。",
  "settings.historyDeleted": "削除しました。",
  "settings.claudeMissing": "見つかりません（PATH を確認してください）",
  "settings.dataDir": "データの保存先",

  "update.title": "アップデート",
  "update.current": "現在 v{version}",
  "update.dev": "開発版のため確認しません",
  "update.checking": "確認しています…",
  "update.upToDate": "最新です",
  "update.available": "v{version} があります",
  "update.downloading": "ダウンロード中… {percent}%",
  "update.ready": "v{version} の準備ができました",
  "update.installing": "Claude を終了して更新しています…",
  "update.error": "確認できませんでした",
  "update.check": "確認",
  "update.download": "ダウンロード",
  "update.install": "再起動して更新",
  "update.installTitle": "開いている Claude を正常に終了させてから、更新して再起動します",
  "update.readyHint": "再起動すると、各ペインの Claude は正常に終了し、更新後に同じ会話で再開します。",
  "update.readyOnExit":
    "v{version} は、次にアプリを閉じたときにインストールされます。作業は中断されません。今すぐ再起動して更新することもできます。",
  "update.installOnExit": "ダウンロードした更新を、アプリを閉じるときにインストールする（起動し直さない）",
  "update.notes": "変更点",

  "history.title": "会話を検索",
  "history.placeholder": "過去の会話を検索（言葉・ファイル名・エラー・フォルダ）",
  "history.indexing": "索引を作成中… {done}/{total}",
  "history.indexed": "{count} 件の会話",
  "history.copyCommand": "コマンドをコピー",
  "history.copied": "コピーしました: {command}",
  "history.folderMissing": "フォルダが見つかりません: {path}",
  "history.paneNotReady": "ペインの起動が間に合いませんでした。もう一度試してください。",
  "history.confirmType":
    "上のペインはすでに動いています。このコマンドを打ち込みますか？\n\n{command}\n\nシェルのプロンプトが出ている状態か確認してください。「いいえ」を選ぶとコマンドをコピーします。",
  "history.consentIntro":
    "Claude Code・Codex CLI・Antigravity CLI との過去の会話を検索して、そのフォルダで再開できるようにします。",
  "history.consentSources":
    "これらのツールがこの PC に保存している会話の記録（~/.claude、~/.codex、~/.gemini/antigravity-cli）を読みます。",
  "history.consentCopy":
    "すばやく検索するため、会話のテキスト（あなたの発言と AI の返答。ツールの出力は除く）のコピーを索引ファイルに持ちます。会話に貼り付けたキーやパスワードなども、このコピーに含まれます。",
  "history.consentLocal": "索引はこの PC のローカル（ローミングしない）データフォルダに置き、外部には送りません。",
  "history.consentDelete": "設定からいつでもオフにして、索引を削除できます。",
  "history.enable": "オンにして索引を作る",

  "notify.permission": "許可を待っています",
  "notify.error": "エラーで止まりました",
  "notify.done": "完了しました",

  "time.now": "たった今",
  "time.minutes": "{n}分前",
  "time.hours": "{n}時間前",
  "time.days": "{n}日前",
  "time.months": "{n}か月前",
  "time.years": "{n}年前",
};

export type Lang = "en" | "ja";
export type LanguageSetting = "auto" | Lang;

export const LANGUAGES: { value: Lang; label: string }[] = [
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
];

const catalogs: Record<Lang, Record<MessageKey, string>> = { en, ja };

export function resolveLang(setting: LanguageSetting | undefined): Lang {
  if (setting === "en" || setting === "ja") return setting;
  const prefs = navigator.languages?.length ? navigator.languages : [navigator.language];
  for (const p of prefs) {
    const base = p.toLowerCase().split("-")[0];
    if (base in catalogs) return base as Lang;
  }
  return "en";
}

export type Vars = Record<string, string | number>;
export type Translate = (key: MessageKey, vars?: Vars) => string;

function format(lang: Lang, key: MessageKey, vars?: Vars): string {
  const text = catalogs[lang][key] ?? en[key];
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (m, name: string) =>
    name in vars ? String(vars[name]) : m,
  );
}

/** Translate outside React (notifications, dialogs). */
export const t: Translate = (key, vars) =>
  format(resolveLang(useStore.getState().settings.language), key, vars);

/** Translate inside React; re-renders when the language changes. */
export function useT(): Translate {
  const lang = useStore((s) => resolveLang(s.settings.language));
  return (key, vars) => format(lang, key, vars);
}

/** The current language, for things like the summary prompt and <html lang>. */
export function currentLang(): Lang {
  return resolveLang(useStore.getState().settings.language);
}
