-- tty アプリと Zed をアクティブにした瞬間、キーボード入力ソースを ABC へ戻す。あわせて、適用で
-- この設定ファイルの内容が変わったときに自身を読み直す。
--
-- 扱う向きは ABC へ戻す側だけで、日本語入力への切り替えは持たない。日本語へは利用者が
-- その都度切り替える。

local ABC_SOURCE_ID = "com.apple.keylayout.ABC"

local INIT_FILE_PATH = hs.configdir .. "/init.lua"

-- 起動時の収束を済ませたプロセスの記録先。`hs.settings` は `org.hammerspoon.Hammerspoon` の
-- defaults へ書くため、下で監視する設定ディレクトリを触らない。
local CONVERGED_PROCESS_KEY = "dotfilesInputSourceConvergedProcessID"

local FORCE_ABC_BUNDLE_IDS = {
  ["com.mitchellh.ghostty"] = true,
  ["com.googlecode.iterm2"] = true,
  ["com.apple.Terminal"] = true,
  ["dev.zed.Zed"] = true,
}

local function forceAbcIfTarget(app)
  if app ~= nil and FORCE_ABC_BUNDLE_IDS[app:bundleID()] then
    hs.keycodes.currentSourceID(ABC_SOURCE_ID)
  end
end

local function readInitFile()
  local file = io.open(INIT_FILE_PATH, "r")
  if file == nil then
    return nil
  end
  local content = file:read("a")
  file:close()
  return content
end

-- watcher は userdata の `__gc` が stop を呼ぶため、global に置いて参照を残す。local にすると
-- この設定を読み終えた時点で回収され、監視が止まる。
inputSourceWatcher = hs.application.watcher.new(function(_, event, app)
  if event == hs.application.watcher.activated then
    forceAbcIfTarget(app)
  end
end)

inputSourceWatcher:start()

-- `start()` は NSWorkspace の observer を登録するだけで、開始時点の前面アプリには callback を出さない。
-- プロセス起動時の前面アプリも同じ処理へ通し、対象アプリが前面なら ABC で始める。
--
-- 通すのはプロセスごとに 1 回だけにする。下の読み直しでも通すと、利用者が自分で選んだ日本語入力を
-- 適用の裏で ABC へ戻してしまう。
if hs.settings.get(CONVERGED_PROCESS_KEY) ~= hs.processInfo.processID then
  hs.settings.set(CONVERGED_PROCESS_KEY, hs.processInfo.processID)
  forceAbcIfTarget(hs.application.frontmostApplication())
end

-- 適用でこのファイルの内容が変わったら設定を読み直す。Hammerspoon は設定ファイルの変更を自分では
-- 読み直さない。
--
-- 監視対象は init.lua ではなく設定ディレクトリにする。`hs.pathwatcher` は渡されたパスを
-- `stringByResolvingSymlinksInPath` で解決してから FSEvents へ渡すため、init.lua を渡すと監視先が
-- リンク先の store path になり、張り替えでは event が出ない。ディレクトリを見れば張り替えは
-- ディレクトリ内の変更として届く。
--
-- ただし適用は内容が同じでもリンクを張り替える。`flake.lock` の bump だけで store path が動き、home 層と
-- darwin 層で 2 回張り替わるため、event の有無ではなく読み込んだ内容との比較で読み直しを決める。
--
-- watcher を global に置く理由は `inputSourceWatcher` と同じ。
local loadedInitFile = readInitFile()

configReloadWatcher = hs.pathwatcher.new(hs.configdir, function()
  local currentInitFile = readInitFile()
  if currentInitFile ~= nil and currentInitFile ~= loadedInitFile then
    hs.reload()
  end
end)

configReloadWatcher:start()
