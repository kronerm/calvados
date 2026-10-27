--!nonstrict
-- The Android notification's controls, laid out for a panel: the batteries,
-- the noise mode as a choice, the two switching settings with what they do,
-- and the firmware (or its update).

local format = require("./lib/format.luau")

local state = noctalia.state.get("calvados.state") or { running = false }

-- The daemon applies it; the next `watch` line re-renders.
local function set(setting, value)
  noctalia.runAsync({ "calvados", "set", setting, value })
end

local function heading(text)
  return ui.label({ text = text, fontWeight = "medium", color = "on_surface_variant" })
end

local function header()
  local running = state.running == true
  local connected = running and state.connected == true
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
  local connected = state.running == true and state.connected == true
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
        ui.box({ width = 16, height = 16 }),
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

local function setting(title, description, checked, key)
  return ui.row({ gap = 12, align = "center" }, {
    ui.column({ gap = 2, flexGrow = 1 }, {
      ui.label({ text = title }),
      ui.label({ text = description, fontSize = 12, color = "on_surface_variant", maxLines = 2 }),
    }),
    ui.toggle({
      checked = checked == true,
      enabled = state.running == true,
      onChange = function(value)
        set(key, value == "true" and "on" or "off")
      end,
    }),
  })
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
  local action = state.connected == true and format.updateAction(state)
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

local function render()
  local rows = {
    header(),
    batteries(),
    noiseControl(),
    ui.column({ gap = 12 }, {
      setting(
        "Automatic switching",
        "Take the AirPods when something starts playing on this computer.",
        state.auto_switch,
        "auto-switch"
      ),
      setting(
        "Automatic connection",
        "Connect when the AirPods come out of the case nearby, even if another device had them last.",
        state.auto_connect,
        "auto-connect"
      ),
    }),
  }
  local line = firmware()
  if line then
    table.insert(rows, line)
  end
  panel.render(ui.column({ flexGrow = 1, gap = 18, align = "stretch" }, rows))
end

noctalia.state.watch("calvados.state", function(value)
  state = value or { running = false }
  render()
end)

function onOpen(_context)
  render()
end
