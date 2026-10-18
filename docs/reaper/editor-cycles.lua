-- Opens and closes a plugin's editor CYCLES times, writes what happened to $OUT/result.txt,
-- then quits Reaper. An example of a driving script: see ../driving-reaper.md.
--
-- Environment (passed with flatpak run --env=...):
--   PLUGIN  the FX to load, as Reaper names it, e.g. "VST3:Doubler" or "CLAP:Doubler"
--   OUT     a folder for results, readable by the agent
--   CYCLES  how many times to open and close the editor (default 20)

local OUT = os.getenv("OUT")
local PLUGIN = os.getenv("PLUGIN")
local CYCLES = tonumber(os.getenv("CYCLES") or "20")
local OPEN_S, CLOSED_S = 1.0, 0.4

local result = io.open(OUT .. "/result.txt", "w")

-- Replace the project with an empty one, so quitting has nothing to save and never prompts.
local function quit()
  result:close()
  local empty = OUT .. "/empty.rpp"
  local file = io.open(empty, "w")
  file:write('<REAPER_PROJECT 0.1 "7.0" 0\n>\n')
  file:close()
  reaper.Main_openProject("noprompt:" .. empty)
  reaper.Main_OnCommand(40004, 0) -- File: Quit REAPER
end

reaper.InsertTrackAtIndex(0, false)
local track = reaper.GetTrack(0, 0)
local fx = reaper.TrackFX_AddByName(track, PLUGIN, false, -1)
result:write("fx ", fx, "\n")
if fx < 0 then
  result:write("error: ", PLUGIN, " did not load\n")
  quit()
  return
end

local cycle, open, next_at = 0, false, reaper.time_precise() + 0.5
local function step()
  if reaper.time_precise() >= next_at then
    if open then
      reaper.TrackFX_Show(track, fx, 2) -- hide the floating window
      open, next_at = false, reaper.time_precise() + CLOSED_S
      if cycle >= CYCLES then
        result:write("cycles ", cycle, "\n")
        quit()
        return
      end
    else
      cycle = cycle + 1
      reaper.TrackFX_Show(track, fx, 3) -- show the floating window
      open, next_at = true, reaper.time_precise() + OPEN_S
    end
  end
  reaper.defer(step)
end
step()
