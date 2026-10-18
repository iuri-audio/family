-- In touch mode, does a gesture on a parameter write a few points and then let go, or stay
-- latched until playback stops? Each phase plays from 0, lets a gesture happen, stops, and writes
-- the parameter's envelope points to $OUT/result.txt. The developer does the gestures on the
-- plugin's editor when prompted. See ../driving-reaper.md.
--
-- Environment (KEY=VALUE arguments to the launcher):
--   PLUGIN    the FX to load, as Reaper names it, e.g. "VST3:Doubler" or "CLAP:Doubler"
--   PARAM     the parameter's name, as the host shows it, e.g. "Width"
--   SELFTEST  1 runs only a scripted gesture (set to 0.9, then end the edit), with no prompts, to
--             check the measuring. A stock plugin run the same way (PLUGIN=ReaEQ
--             "PARAM=Gain-Low Shelf") shows what Reaper itself records.

local PLUGIN, PARAM = os.getenv("PLUGIN"), os.getenv("PARAM")
local SELFTEST = os.getenv("SELFTEST") == "1"

local drive = dofile(os.getenv("PRELUDE"))

reaper.InsertTrackAtIndex(0, false)
local track = reaper.GetTrack(0, 0)
local fx = reaper.TrackFX_AddByName(track, PLUGIN, false, -1)
local param
for index = 0, reaper.TrackFX_GetNumParams(track, fx) - 1 do
  local _, name = reaper.TrackFX_GetParamName(track, fx, index)
  if name == PARAM then param = index end
end
drive.say(PLUGIN, ": fx ", fx, ", ", PARAM, " is parameter ", tostring(param))
if fx < 0 or not param then
  drive.quit()
  return
end

local env = reaper.GetFXEnvelope(track, fx, param, true)
local _, chunk = reaper.GetEnvelopeStateChunk(env, "", false)
reaper.SetEnvelopeStateChunk(env, (chunk:gsub("\nARM %d", "\nARM 1")), false)
reaper.SetTrackAutomationMode(track, 2) -- touch
reaper.TrackFX_Show(track, fx, 3)

local phases
if SELFTEST then
  phases = { { name = "scripted gesture", length = 6, scripted = true } }
else
  phases = {
    { name = "one wheel notch", length = 10,
      prompt = "After OK, playback starts.\n\nWithin 6 s: move the mouse over " .. PARAM
        .. " and scroll ONE notch. Then take your hand off the mouse.\n\nPlayback stops by itself after 10 s." },
    { name = "double-click", length = 10,
      prompt = "After OK, playback starts.\n\nWithin 6 s: double-click " .. PARAM
        .. " (it resets to its default). Then take your hand off the mouse.\n\nPlayback stops by itself after 10 s." },
    { name = "editor closed mid-drag", length = 10, close_at = 5,
      prompt = "After OK, playback starts.\n\nRight away: press on " .. PARAM
        .. " and drag up and down, and KEEP HOLDING the button. The editor closes by itself 5 s after OK.\n\nRelease the button once it's gone. Playback stops by itself after 10 s." },
  }
end

local function analyse(phase, stopped_at, closed_at)
  local count = reaper.CountEnvelopePoints(env)
  drive.say("")
  drive.say("== ", phase.name)
  if count == 0 then
    drive.say("no points written")
    return
  end
  local points, first, last = {}, nil, nil
  for i = 0, count - 1 do
    local _, time, value = reaper.GetEnvelopePoint(env, i)
    first = first or time
    last = time
    points[#points + 1] = string.format("%.2f@%.2f", value, time)
  end
  drive.say(string.format("%d points from %.2f s to %.2f s; playback stopped at %.2f s%s",
    count, first, last, stopped_at,
    closed_at and string.format("; editor closed at %.2f s", closed_at) or ""))
  drive.say("points (value@time): ", table.concat(points, " "))
  drive.say(string.format("%s reads %.3f (%s) at the stop", PARAM,
    reaper.TrackFX_GetParamNormalized(track, fx, param),
    select(2, reaper.TrackFX_GetFormattedParamValue(track, fx, param, ""))))
  local latched = last > stopped_at - 0.5
  drive.say(latched and "LATCHED: writing went on until playback stopped"
    or string.format("let go: writing stopped %.2f s before playback did", stopped_at - last))
end

local index, phase, started, closed_at, scripted_done = 0, nil, nil, nil, false
local function next_phase()
  index = index + 1
  phase = phases[index]
  if not phase then
    reaper.TrackFX_Show(track, fx, 2)
    drive.quit()
    return false
  end
  reaper.DeleteEnvelopePointRange(env, -1, 1e9)
  reaper.Envelope_SortPoints(env)
  if phase.prompt then
    reaper.MB(phase.prompt, "touch: " .. phase.name .. " (" .. PLUGIN .. ")", 0)
  end
  reaper.SetEditCurPos(0, false, false)
  reaper.OnPlayButton()
  started, closed_at, scripted_done = reaper.time_precise(), nil, false
  return true
end

local function step()
  local elapsed = reaper.time_precise() - started
  if phase.scripted and not scripted_done and elapsed > 1 then
    reaper.TrackFX_SetParam(track, fx, param, 0.9)
    reaper.TrackFX_EndParamEdit(track, fx, param)
    scripted_done = true
  end
  if phase.close_at and not closed_at and elapsed >= phase.close_at then
    reaper.TrackFX_Show(track, fx, 2)
    closed_at = reaper.GetPlayPosition()
  end
  if elapsed >= phase.length then
    local stopped_at = reaper.GetPlayPosition()
    reaper.OnStopButton()
    analyse(phase, stopped_at, closed_at)
    if not next_phase() then return end
  end
  reaper.defer(step)
end

drive.start(function()
  if next_phase() then step() end
end)
