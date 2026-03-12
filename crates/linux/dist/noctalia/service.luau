--!nonstrict
-- The only entry that talks to the daemon: `calvados watch` prints its state
-- as one JSON object per line on every change, republished here for the bar
-- and the panel.

local STATE = "calvados.state"

if not noctalia.commandExists("calvados") then
  noctalia.state.set(STATE, { running = false, missing = true })
  return
end

local function onLine(line)
  local state = noctalia.json.decode(line)
  if type(state) == "table" then
    noctalia.state.set(STATE, state)
  end
end

if not noctalia.runStream("calvados watch", onLine) then
  noctalia.log("calvados: couldn't start `calvados watch`")
end
