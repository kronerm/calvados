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

function format.ownership(state)
  if state.ownership == "owning" then
    return "Active"
  elseif state.ownership == "claiming" then
    return "Taking…"
  end
  return "Standby"
end

return format
