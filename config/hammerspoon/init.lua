-- tty アプリと Zed をアクティブにした瞬間、キーボード入力ソースを ABC へ戻す。
--
-- 扱う向きは ABC へ戻す側だけで、日本語入力への切り替えは持たない。日本語へは利用者が
-- その都度切り替える。

local ABC_SOURCE_ID = "com.apple.keylayout.ABC"

local FORCE_ABC_BUNDLE_IDS = {
  ["com.mitchellh.ghostty"] = true,
  ["com.googlecode.iterm2"] = true,
  ["com.apple.Terminal"] = true,
  ["dev.zed.Zed"] = true,
}

-- watcher は userdata の `__gc` が stop を呼ぶため、global に置いて参照を残す。local にすると
-- この設定を読み終えた時点で回収され、監視が止まる。
inputSourceWatcher = hs.application.watcher.new(function(_, event, app)
  if event ~= hs.application.watcher.activated or app == nil then
    return
  end

  if FORCE_ABC_BUNDLE_IDS[app:bundleID()] then
    hs.keycodes.currentSourceID(ABC_SOURCE_ID)
  end
end)

inputSourceWatcher:start()
