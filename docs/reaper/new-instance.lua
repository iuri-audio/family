-- Save one plugin instance, reload its project, then add new instances in that process.
-- Run with run-wsl-new-instance.sh: its observer acknowledges each settled window before
-- this script hides it. PLUGIN selects VST3/CLAP; NEW_INSTANCES defaults to 3.
-- Each run measures 2 + 2 * NEW_INSTANCES phases, without playing audio.
local OUT, PLUGIN = os.getenv("OUT"), os.getenv("PLUGIN")
local drive = dofile(os.getenv("PRELUDE"))
local NEW_INSTANCES = tonumber(os.getenv("NEW_INSTANCES") or "3")
local plugin_name = (PLUGIN or ""):match("^[^:]+:%s*(.-)%s*$")
local finished = false
local function write(name, value)
  local file = assert(io.open(OUT .. "/" .. name, "w"))
  file:write(value)
  file:close()
end
local function stop(message)
  if finished then return end
  finished = true
  drive.say(message)
  write("finished.txt", message)
  drive.quit()
end
local function safe(fn)
  if finished then return end
  local ok, err = xpcall(fn, debug.traceback)
  if not ok then stop("ERROR: " .. err) end
end
local function later(seconds, fn)
  local until_at = reaper.time_precise() + seconds
  local function tick()
    if finished then return end
    if reaper.time_precise() >= until_at then safe(fn) else reaper.defer(tick) end
  end
  reaper.defer(tick)
end
local function new_instance(label)
  local index = reaper.CountTracks(0)
  reaper.InsertTrackAtIndex(index, false)
  local track = reaper.GetTrack(0, index)
  reaper.GetSetMediaTrackInfo_String(track, "P_NAME", label, true)
  local fx = reaper.TrackFX_AddByName(track, PLUGIN, false, -1)
  assert(fx >= 0, "plugin did not load: " .. PLUGIN)
  return track, fx
end
local function capture(track, fx, label, next_step)
  reaper.TrackFX_Show(track, fx, 3)
  later(2, function()
    assert(reaper.TrackFX_GetFloatingWindow(track, fx), "no floating window: " .. label)
    drive.say("capture ", label)
    write("capture-request.txt", label)
    local deadline = reaper.time_precise() + 15
    local function acknowledge()
      safe(function()
        local file = io.open(OUT .. "/capture-done.txt", "r")
        local ack = file and file:read("*a") or ""
        if file then file:close() end
        if ack:match("^%s*(.-)%s*$") == label then
          reaper.TrackFX_Show(track, fx, 2)
          later(0.3, next_step)
        elseif reaper.time_precise() > deadline then
          stop("ERROR: measurement timeout: " .. label)
        else reaper.defer(acknowledge) end
      end)
    end
    reaper.defer(acknowledge)
  end)
end
local function add_next(number)
  if number > NEW_INSTANCES then
    stop("COMPLETE: saved-project scenario; new instances checked with reopens: " .. NEW_INSTANCES)
    return
  end
  local track, fx = new_instance("new-instance-" .. number)
  capture(track, fx, "new-" .. number .. "-first", function()
    capture(track, fx, "new-" .. number .. "-reopen", function() add_next(number + 1) end)
  end)
end
drive.start(function()
  safe(function()
    assert(plugin_name and plugin_name ~= "", "set PLUGIN to a format-qualified plugin name")
    assert(NEW_INSTANCES and NEW_INSTANCES >= 1 and NEW_INSTANCES % 1 == 0,
      "NEW_INSTANCES must be a positive integer")
    drive.say("Reaper ", reaper.GetAppVersion(), "; plugin ", PLUGIN)
    for i = 0, 10000 do
      local ok, name, ident = reaper.EnumInstalledFX(i)
      if not ok then break end
      if name:find(plugin_name, 1, true) then drive.say("installed ", name, ": ", ident) end
    end
    local track, fx = new_instance("saved-instance")
    capture(track, fx, "seed-first", function()
      reaper.Main_SaveProjectEx(0, OUT .. "/fixture.rpp", 8)
      local file = assert(io.open(OUT .. "/blank.rpp", "w"))
      file:write('<REAPER_PROJECT 0.1 "7.0" 0\n>\n')
      file:close()
      reaper.Main_openProject("noprompt:" .. OUT .. "/blank.rpp")
      reaper.Main_openProject("noprompt:" .. OUT .. "/fixture.rpp")
      later(1, function()
        local loaded = assert(reaper.GetTrack(0, 0), "saved track did not load")
        assert(reaper.TrackFX_GetCount(loaded) == 1, "saved plugin did not load")
        capture(loaded, 0, "loaded-existing", function() add_next(1) end)
      end)
    end)
  end)
end)
