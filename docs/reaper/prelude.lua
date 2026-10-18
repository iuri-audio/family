-- What every driving script needs (see ../driving-reaper.md). The launcher passes this file's
-- path as PRELUDE:
--
--     local drive = dofile(os.getenv("PRELUDE"))
--     drive.start(function()
--       drive.say("...")
--       drive.quit()
--     end)

local drive = {}

local OUT = os.getenv("OUT")
local result = io.open(OUT .. "/result.txt", "w")
local launched = reaper.time_precise()

-- Playback started in Reaper's first seconds stops at once at 0 s (seen on Linux with JACK),
-- though Audio_IsRunning already says yes.
local SETTLE_S = 3

-- Writes its arguments and a newline to $OUT/result.txt, flushed so a run that hangs still leaves
-- what it saw.
function drive.say(...)
  result:write(...)
  result:write("\n")
  result:flush()
end

local finished = false

-- Writes `verdict` (PASS, FAIL or ERROR) and a reason to $OUT/verdict.txt and result.txt, then
-- quits. Only the first verdict counts.
local function conclude(verdict, ...)
  if finished then return end
  local reason = table.concat({ ... })
  drive.say(verdict, ": ", reason)
  local file = io.open(OUT .. "/verdict.txt", "w")
  file:write(verdict, "\n", reason, "\n")
  file:close()
  drive.quit()
end

-- For checks run by `cargo xtask e2e`: the plugin did what the check expects.
function drive.pass(...) conclude("PASS", ...) end

-- The plugin did something else: a bug.
function drive.fail(...) conclude("FAIL", ...) end

-- The check couldn't run, so it says nothing about the plugin, e.g. Reaper has no audio device.
function drive.error(...) conclude("ERROR", ...) end

-- Like reaper.defer, but a Lua error ends the run with an ERROR verdict instead of opening
-- Reaper's error dialog, which would hold the run until the launcher's timeout.
function drive.defer(fn)
  reaper.defer(function()
    if finished then return end
    local ok, err = xpcall(fn, debug.traceback)
    if not ok then drive.error(err) end
  end)
end

-- Calls `fn` once Reaper has settled enough to play.
function drive.start(fn)
  local function wait()
    if reaper.time_precise() - launched < SETTLE_S then
      drive.defer(wait)
    else
      fn()
    end
  end
  drive.defer(wait)
end

-- Calls `fn` after `seconds`, without blocking Reaper.
function drive.after(seconds, fn)
  local at = reaper.time_precise() + seconds
  local function wait()
    if reaper.time_precise() < at then drive.defer(wait) else fn() end
  end
  drive.defer(wait)
end

-- The index and name of the parameter a check exercises: the one named in $PARAM, else the first
-- continuous one (a stepped one can't follow a ramp), else the first.
function drive.param(track, fx)
  local wanted, first = os.getenv("PARAM"), nil
  for index = 0, reaper.TrackFX_GetNumParams(track, fx) - 1 do
    local _, name = reaper.TrackFX_GetParamName(track, fx, index)
    if wanted then
      if name == wanted then return index, name end
    elseif not reaper.TrackFX_GetParameterStepSizes(track, fx, index) then
      return index, name
    end
    first = first or index
  end
  if wanted or not first then return nil end
  return first, select(2, reaper.TrackFX_GetParamName(track, fx, first))
end

-- Replaces the project with an empty one, so quitting has nothing to save and never prompts, and
-- quits Reaper.
function drive.quit()
  if finished then return end
  finished = true
  result:close()
  local empty = OUT .. "/empty.rpp"
  local file = io.open(empty, "w")
  file:write('<REAPER_PROJECT 0.1 "7.0" 0\n>\n')
  file:close()
  reaper.Main_openProject("noprompt:" .. empty)
  reaper.Main_OnCommand(40004, 0) -- File: Quit REAPER
end

return drive
