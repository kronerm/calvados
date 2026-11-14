--!nonstrict
-- The Android notification's controls, laid out for a panel: the batteries,
-- the noise mode as a choice, the two switching settings and the AirPods'
-- main ones with what they do, "More" (the AirPods' other settings and the
-- rename), and the firmware (or its update).

local format = require("./lib/format.luau")

local state = noctalia.state.get(format.STATE) or { running = false }
local render

-- The daemon applies it; the next `watch` line re-renders.
local function set(setting, value)
  noctalia.runAsync({ "calvados", "set", setting, value })
end

local function heading(text)
  return ui.label({ text = text, fontWeight = "medium", color = "on_surface_variant" })
end

local function header()
  local running = state.running == true
  local connected = format.connected(state)
  local title = not running and "Calvados isn't running"
    or (state.name ~= "" and state.name or "AirPods")
  local status = not running and "" or (connected and format.ownership(state) or "Not connected")
  return ui.row({ gap = 10, align = "center" }, {
    ui.glyph({
      name = "device-airpods",
      size = 22,
      color = state.ownership == "owning" and "primary" or "on_surface",
    }),
    ui.label({ text = title, fontSize = 16, fontWeight = "bold", flexGrow = 1 }),
    ui.label({
      text = status,
      color = state.ownership == "owning" and "primary" or "on_surface_variant",
    }),
  })
end

-- A column per component; `placeGlyph` (buds only) marks where it is.
local function battery(label, name, placeGlyph)
  local component = format.component(state, name)
  if component == nil then
    return ui.column({ flexGrow = 1, gap = 4, align = "center" }, {
      ui.label({ text = label, color = "on_surface_variant" }),
      ui.label({ text = "—" }),
    })
  end
  local row = {
    ui.glyph({
      name = format.batteryGlyph(component),
      color = component.charging and "primary" or "on_surface",
      opacity = component.last_known and 0.5 or 1,
    }),
    ui.label({ text = format.level(component) }),
  }
  local mark = placeGlyph and placeGlyph(component)
  if mark then
    table.insert(row, ui.glyph({ name = mark, color = "on_surface_variant" }))
  end
  -- The primary bud (it holds the link): a dot by its name.
  local title = { ui.label({ text = label, color = "on_surface_variant" }) }
  if component.primary then
    table.insert(title, ui.glyph({ name = "point-filled", size = 10, color = "primary" }))
  end
  return ui.column({
    flexGrow = 1,
    gap = 4,
    align = "center",
  }, {
    ui.row({ gap = 2, align = "center" }, title),
    ui.row({ gap = 4, align = "center" }, row),
  })
end

local function budPlace(component)
  if component.place == "in-ear" then
    return "ear"
  elseif component.place == "in-case" then
    return "device-airpods-case"
  end
  return nil
end

local function batteries()
  return ui.row({ gap = 8 }, {
    battery("Left", "left", budPlace),
    battery("Right", "right", budPlace),
    battery("Case", "case", nil),
  })
end

-- The dot in a tile's corner: in the press-and-hold cycle or not; a click
-- toggles it (the tile's own click sets the mode).
local function cycleDot(mode, connected, color)
  local glyph, opacity = format.cycleDot(state, mode.name)
  local toggles = connected and format.cycleToggles(state, mode.name)
  return ui.row({
    key = `{mode.name}:{glyph}:{opacity}:{toggles}`,
    width = 16,
    height = 16,
    align = "center",
    justify = "center",
    onClick = toggles and function()
      noctalia.runAsync({ "calvados", "set", "cycle", "toggle", mode.name })
    end or nil,
  }, { ui.glyph({ name = glyph, size = 12, color = color, opacity = opacity }) })
end

-- Tiles, glyph above label, so "Transparency" fits a third of the panel;
-- the current mode is filled, and a dot beside the glyph (balanced on the
-- other side) marks the press-and-hold cycle. The key changes with the
-- selection: the host diffs retained nodes and keeps a dropped prop (the
-- old fill), so a tile whose selection changed is built anew instead.
local function noiseControl()
  local connected = format.connected(state)
  local tiles = {}
  for _, mode in ipairs(format.MODES) do
    local selected = state.noise_mode == mode.name
    local color = selected and "on_primary" or "on_surface"
    table.insert(tiles, ui.column({
      key = `{mode.name}:{selected}`,
      flexGrow = 1,
      gap = 6,
      align = "stretch",
      paddingV = 10,
      paddingH = 6,
      radius = 10,
      fill = selected and "primary" or nil,
      border = selected and nil or "outline",
      borderWidth = 1,
      opacity = connected and 1 or 0.5,
      onClick = connected and function()
        set("noise", mode.name)
      end or nil,
    }, {
      ui.row({ align = "start" }, {
        ui.row({ width = 16, height = 16 }),
        ui.row({ flexGrow = 1, justify = "center" }, {
          ui.glyph({ name = mode.glyph, size = 20, color = color }),
        }),
        cycleDot(mode, connected, color),
      }),
      ui.label({ text = mode.short, fontSize = 12, color = color, maxLines = 1, textAlign = "center" }),
    }))
  end
  -- Off (never offered, but the pods can report it) or not yet known: say so.
  local title = "Noise control"
  if connected and state.noise_mode == "off" then
    title = "Noise control · Off"
  elseif connected and (state.noise_mode or "") == "" then
    title = "Noise control · unknown yet"
  end
  return ui.column({ gap = 8 }, { heading(title), ui.row({ gap = 8 }, tiles) })
end

-- A title, what it does (optional) and a switch.
local function setting(title, description, checked, enabled, onChange)
  local text = { ui.label({ text = title }) }
  if description then
    table.insert(text, ui.label({ text = description, fontSize = 12, color = "on_surface_variant", maxLines = 2 }))
  end
  return ui.row({ gap = 12, align = "center" }, {
    ui.column({ gap = 2, flexGrow = 1 }, text),
    ui.toggle({ checked = checked == true, enabled = enabled, onChange = onChange }),
  })
end

local function setPod(name, value)
  noctalia.runAsync({ "calvados", "set", "pod", name, value })
end

-- A calvados setting, stored by the daemon.
local function hostSetting(title, description, checked, key)
  return setting(title, description, checked, state.running == true, function(value)
    set(key, value == "true" and "on" or "off")
  end)
end

-- One of the AirPods' own settings with a row of its own; nil until they
-- report it.
local function podSetting(entry)
  local value = format.podValue(state, entry.name)
  if value == nil then
    return nil
  end
  return setting(entry.title, entry.description, value == "on", true, function(checked)
    setPod(entry.name, checked == "true" and "on" or "off")
  end)
end

-- A choice as segments, the current one filled (keyed by it, as the noise
-- tiles are).
local function choice(entry, value)
  local segments = {}
  for _, option in ipairs(entry.options) do
    local selected = option == value
    table.insert(segments, ui.row({
      key = `{entry.name}:{option}:{selected}`,
      flexGrow = 1,
      justify = "center",
      paddingV = 5,
      radius = 8,
      fill = selected and "primary" or nil,
      border = selected and nil or "outline",
      borderWidth = 1,
      onClick = not selected and function()
        setPod(entry.name, option)
      end or nil,
    }, {
      ui.label({
        text = format.podValueLabel(option),
        fontSize = 12,
        maxLines = 1,
        color = selected and "on_primary" or "on_surface",
      }),
    }))
  end
  return ui.row({ gap = 12, align = "center" }, {
    ui.label({ text = entry.title, flexGrow = 1, maxLines = 1 }),
    ui.row({ gap = 4, width = 210 }, segments),
  })
end

-- The adaptive level: a slider, sent as it moves (in tens, so a drag sends
-- a few commands, not one per pixel).
local sentLevel = nil

local function level(entry, value)
  return ui.column({ gap = 4 }, {
    ui.row({ align = "center" }, {
      ui.label({ text = entry.title, flexGrow = 1 }),
      ui.label({ text = format.podValueLabel(value), color = "on_surface_variant" }),
    }),
    ui.slider({
      min = 0,
      max = 100,
      step = 10,
      value = tonumber(value) or 50,
      controlSize = "sm",
      onChange = function(moved)
        local rounded = tostring(math.floor((tonumber(moved) or 0) / 10 + 0.5) * 10)
        if rounded ~= sentLevel and rounded ~= value then
          sentLevel = rounded
          setPod(entry.name, rounded)
        end
      end,
    }),
    ui.row({}, {
      ui.label({ text = entry.low, fontSize = 11, color = "on_surface_variant", flexGrow = 1 }),
      ui.label({ text = entry.high, fontSize = 11, color = "on_surface_variant" }),
    }),
  })
end

-- A setting under "More", as its kind wants it.
local function moreRow(entry, value)
  if entry.kind == "choice" then
    return choice(entry, value)
  elseif entry.kind == "level" then
    return level(entry, value)
  end
  return setting(entry.title, entry.description, value == "on", true, function(checked)
    setPod(entry.name, checked == "true" and "on" or "off")
  end)
end

-- "More": the AirPods' other settings, title and value, a click stepping to
-- the next value, and the rename field. Collapsed on every open.
local moreOpen = false
local draftName = nil

local function rename()
  local name = draftName
  if name and #name >= 1 and #name <= 32 then
    noctalia.runAsync({ "calvados", "rename", name })
  end
end

local function more()
  local children = {
    ui.row({
      key = `more:{moreOpen}`,
      gap = 6,
      align = "center",
      onClick = function()
        moreOpen = not moreOpen
        render()
      end,
    }, {
      ui.label({ text = "More", fontWeight = "medium", color = "on_surface_variant", flexGrow = 1 }),
      ui.glyph({ name = moreOpen and "chevron-up" or "chevron-down", size = 16, color = "on_surface_variant" }),
    }),
  }
  if not moreOpen then
    return ui.column({ gap = 8 }, children)
  end
  local rows = {}
  for _, entry in ipairs(format.POD_SETTINGS) do
    local value = not entry.main and format.podValue(state, entry.name)
    if value then
      table.insert(rows, moreRow(entry, value))
    end
  end
  table.insert(rows, ui.row({ gap = 8, align = "center" }, {
    ui.input({
      key = "rename",
      value = state.name or "",
      placeholder = "Name",
      controlSize = "sm",
      flexGrow = 1,
      onChange = function(text)
        draftName = text
      end,
      onSubmit = function(text)
        draftName = text
        rename()
      end,
    }),
    ui.row({
      paddingH = 10,
      paddingV = 4,
      radius = 8,
      border = "outline",
      borderWidth = 1,
      onClick = rename,
    }, { ui.label({ text = "Rename", fontSize = 12, maxLines = 1 }) }),
  }))
  table.insert(children, ui.column({ gap = 10 }, rows))
  return ui.column({ gap = 8 }, children)
end

-- One small line: a firmware update while there is one, else the firmware
-- (only while connected; the daemon clears it otherwise), with the update's
-- button when there's something to start or cancel.
local function firmware()
  local text = format.update(state) or format.firmware(state)
  if not text then
    return nil
  end
  local line = ui.label({ text = text, fontSize = 12, color = "on_surface_variant", maxLines = 1, flexGrow = 1 })
  local action = format.connected(state) and format.updateAction(state)
  if not action then
    return line
  end
  local button = ui.row({
    key = action,
    paddingH = 10,
    paddingV = 4,
    radius = 8,
    border = "outline",
    borderWidth = 1,
    onClick = function()
      noctalia.runAsync({ "calvados", "firmware", action == "Update" and "update" or "cancel" })
    end,
  }, { ui.label({ text = action, fontSize = 12, maxLines = 1 }) })
  return ui.row({ gap = 12, align = "center" }, { line, button })
end

render = function()
  local settings = {
    hostSetting(
      "Automatic switching",
      "Take the AirPods when something starts playing on this computer.",
      state.auto_switch,
      "auto-switch"
    ),
    hostSetting(
      "Automatic connection",
      "Connect when the AirPods come out of the case nearby, even if another device had them last.",
      state.auto_connect,
      "auto-connect"
    ),
  }
  for _, entry in ipairs(format.POD_SETTINGS) do
    local row = entry.main and podSetting(entry)
    if row then
      table.insert(settings, row)
    end
  end
  local rows = {
    header(),
    batteries(),
    noiseControl(),
    ui.column({ gap = 12 }, settings),
  }
  if format.connected(state) then
    table.insert(rows, more())
  end
  local line = firmware()
  if line then
    table.insert(rows, line)
  end
  local content = ui.column({ flexGrow = 1, gap = 18, align = "stretch" }, rows)
  -- With More open the panel is at its cap and scrolls. A scroll fills
  -- the panel, so the panel can't shrink around one: closed, it's gone.
  if moreOpen then
    content = ui.scroll({ key = "scroll", flexGrow = 1 }, { content })
  end
  panel.render(content)
end

noctalia.state.watch(format.STATE, function(value)
  state = value or { running = false }
  render()
end)

function onOpen(_context)
  moreOpen = false
  draftName = nil
  sentLevel = nil
  render()
end
