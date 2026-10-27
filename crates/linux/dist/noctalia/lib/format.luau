--!nonstrict
-- Shared by the bar and the panel: glyphs and labels from `calvados watch`'s
-- state (Tabler glyphs, shipped with Noctalia).

local format = {}

format.MODES = {
  { name = "anc", label = "Noise cancelling", short = "ANC", glyph = "ear-off" },
  { name = "transparency", label = "Transparency", short = "Transparency", glyph = "ear" },
  { name = "adaptive", label = "Adaptive", short = "Adaptive", glyph = "ear-scan" },
}

-- The component named `name` ("left", "right", "case"), or nil.
function format.component(state, name)
  for _, component in ipairs(state.buds or {}) do
    if component.component == name then
      return component
    end
  end
  return nil
end

function format.batteryGlyph(component)
  if component.charging then
    return "battery-charging"
  end
  local level = component.level
  if level >= 88 then
    return "battery-4"
  elseif level >= 63 then
    return "battery-3"
  elseif level >= 38 then
    return "battery-2"
  elseif level >= 13 then
    return "battery-1"
  end
  return "battery"
end

-- `80%`, or `(80%)` when last known.
function format.level(component)
  return component.last_known and `({component.level}%)` or `{component.level}%`
end

-- Where a bud is: "In ear", "In case", "Out".
function format.place(component)
  if component.place == "in-ear" then
    return "In ear"
  elseif component.place == "in-case" then
    return "In case"
  end
  return "Out"
end

-- The live readings, left, right, case, as `{ name, component }`, for the bar.
function format.liveLevels(state)
  local levels = {}
  for _, name in ipairs({ "left", "right", "case" }) do
    local component = format.component(state, name)
    if component and not component.last_known then
      table.insert(levels, { name = name, component = component })
    end
  end
  return levels
end

-- The noise mode's label: "Noise cancelling", …, "Off", or "—" while unknown.
function format.mode(state)
  for _, mode in ipairs(format.MODES) do
    if mode.name == state.noise_mode then
      return mode.label
    end
  end
  return state.noise_mode == "off" and "Off" or "—"
end

-- "Firmware 7.1.21" (UARP) or "Firmware 7.1 (6663)" (0x1D alone), plus
-- " · 9.0.348 available" when Apple has a newer one; nil until reported.
function format.firmware(state)
  if (state.firmware or "") == "" then
    return nil
  end
  local latest = state.firmware_update and string.match(state.latest_firmware or "", "^%S+")
  return latest and `Firmware {state.firmware} · {latest} available` or `Firmware {state.firmware}`
end

-- An update's line, as the Android notification has it (protocol's
-- `display::update`): "Updating firmware… 40 %", "Staged: close the lid", …;
-- nil without one.
local function updateLine(update)
  local name = update and update.state or ""
  if name == "downloading" then
    return "Downloading firmware…"
  elseif name == "offered" then
    return "Offering firmware…"
  elseif name == "deferred" then
    return "Waiting for the AirPods…"
  elseif name == "transferring" then
    return `Updating firmware… {update.progress} %`
  elseif name == "interrupted" then
    return `Update paused at {update.progress} %`
  elseif name == "staged" then
    return "Staged: close the lid"
  elseif name == "done" then
    return `Updated to {update.detail}`
  elseif name == "failed" then
    return `Update failed: {update.detail}`
  end
  return nil
end

-- This host's update, else another calvados host's (" · <its address>").
function format.update(state)
  local line = updateLine(state.update)
  if line then
    return line
  end
  local peer = state.peer_update
  line = updateLine(peer)
  return line and `{line} · {peer.host}` or nil
end

function format.ownership(state)
  if state.ownership == "owning" then
    return "Active"
  elseif state.ownership == "claiming" then
    return "Taking…"
  end
  return "Standby"
end

return format
