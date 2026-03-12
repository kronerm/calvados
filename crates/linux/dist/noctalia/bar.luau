--!nonstrict
-- Bar widget: the AirPods glyph (primary while this host has the audio,
-- dimmed when they're away), the buds' levels as worn, left then right, the
-- primary bud's in bold, and the case's after its glyph while it reports
-- one; the tooltip has the rest. A click opens the panel.

local format = require("./lib/format.luau")

local state = noctalia.state.get("calvados.state") or { running = false }

local function tooltip()
  if state.running ~= true then
    return state.missing and "calvados isn't installed" or "Calvados isn't running"
  end
  if state.connected ~= true then
    return "AirPods not connected"
  end
  local rows = { { state.name ~= "" and state.name or "AirPods", format.ownership(state) } }
  for _, name in ipairs({ "left", "right", "case" }) do
    local component = format.component(state, name)
    if component then
      local detail = name == "case" and ""
        or ` · {format.place(component)}{component.primary and " · primary" or ""}`
      local charging = component.charging and " · charging" or ""
      local label = name == "case" and "Case" or (name == "left" and "Left" or "Right")
      table.insert(rows, { label, `{format.level(component)}{charging}{detail}` })
    end
  end
  table.insert(rows, { "Noise", format.mode(state) })
  return rows
end

local function render()
  local connected = state.running == true and state.connected == true
  local glyph = ui.glyph({
    name = "device-airpods",
    color = state.ownership == "owning" and "primary" or "on_surface",
    opacity = connected and 1 or 0.5,
  })
  local levels = connected and format.liveLevels(state) or {}
  if #levels == 0 or barWidget.isVertical() then
    barWidget.render(glyph)
  else
    local children = { glyph }
    for _, entry in ipairs(levels) do
      local component = entry.component
      if entry.name == "case" then
        table.insert(children, ui.glyph({ name = "device-airpods-case", opacity = 0.7 }))
      end
      table.insert(children, ui.label({
        text = `{component.level}`,
        fontWeight = component.primary and "bold" or "normal",
        maxLines = 1,
      }))
      if component.charging then
        table.insert(children, ui.glyph({ name = "bolt", color = "primary" }))
      end
    end
    barWidget.render(ui.row({ gap = 4, align = "center" }, children))
  end
  barWidget.setTooltip(tooltip())
end

noctalia.state.watch("calvados.state", function(value)
  state = value or { running = false }
  render()
end)

function update()
  noctalia.setUpdateInterval(60000)
  render()
end

function onClick()
  noctalia.togglePanel("kroner/calvados:panel")
end
