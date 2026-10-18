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

-- Calls `fn` once Reaper has settled enough to play.
function drive.start(fn)
  local function wait()
    if reaper.time_precise() - launched < SETTLE_S then
      reaper.defer(wait)
    else
      fn()
    end
  end
  wait()
end

-- Replaces the project with an empty one, so quitting has nothing to save and never prompts, and
-- quits Reaper.
function drive.quit()
  result:close()
  local empty = OUT .. "/empty.rpp"
  local file = io.open(empty, "w")
  file:write('<REAPER_PROJECT 0.1 "7.0" 0\n>\n')
  file:close()
  reaper.Main_openProject("noprompt:" .. empty)
  reaper.Main_OnCommand(40004, 0) -- File: Quit REAPER
end

return drive
