-- Opens and closes a plugin's editor CYCLES times, writes what happened to $OUT/result.txt,
-- then quits Reaper. An example of a driving script: see ../driving-reaper.md.
--
-- Environment (KEY=VALUE arguments to the launcher):
--   PLUGIN  the FX to load, as Reaper names it, e.g. "VST3:Doubler" or "CLAP:Doubler"
--   CYCLES  how many times to open and close the editor (default 20)

local PLUGIN = os.getenv("PLUGIN")
local CYCLES = tonumber(os.getenv("CYCLES") or "20")
local OPEN_S, CLOSED_S = 1.0, 0.4

local drive = dofile(os.getenv("PRELUDE"))

reaper.InsertTrackAtIndex(0, false)
local track = reaper.GetTrack(0, 0)
local fx = reaper.TrackFX_AddByName(track, PLUGIN, false, -1)
drive.say("fx ", fx)
if fx < 0 then
  drive.say("error: ", PLUGIN, " did not load")
  drive.quit()
  return
end

local cycle, open, next_at = 0, false, reaper.time_precise() + 0.5
local function step()
  if reaper.time_precise() >= next_at then
    if open then
      reaper.TrackFX_Show(track, fx, 2) -- hide the floating window
      open, next_at = false, reaper.time_precise() + CLOSED_S
      if cycle >= CYCLES then
        drive.say("cycles ", cycle)
        drive.quit()
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
drive.start(step)
