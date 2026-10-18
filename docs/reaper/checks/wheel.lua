-- e2e-env: FAMILY_E2E_WHEEL=8
--
-- A check run by `cargo xtask e2e` (see ../../driving-reaper.md), needing the e2e build's wheel
-- hook (crates/ui/src/e2e.rs), which the line above turns on: in touch mode, during playback,
-- with flat automation under a parameter, in each format:
--
-- - burst: a quick burst of wheel notches on the dial is recorded whole, and once the gesture
--   ends the parameter returns to the automation. Family issue 2: only the first notches were
--   recorded.
-- - closed: closing the editor partway through the burst ends the gesture, so the parameter
--   returns to the automation instead of staying latched.
--
-- Environment (set by `cargo xtask e2e`):
--   PLUGIN_NAME  the plugin's name, e.g. "Doubler"
--   PARAM        optional: the parameter to exercise, as the host names it; it needs a dial

local drive = dofile(os.getenv("PRELUDE"))
local NAME = os.getenv("PLUGIN_NAME")
local NOTCHES = tonumber(os.getenv("FAMILY_E2E_WHEEL"))

-- The automation's level, and how far each notch moves the dial (family-ui's WHEEL_STEP).
local FLAT, STEP = 0.5, 0.05
-- How long each scenario plays: the hook starts 2 s after the editor opens and the burst lasts
-- half a second, then the gesture ends after 150 ms idle.
local PLAY_S = 6

local scenarios = {}
for _, format in ipairs({ "VST3", "CLAP" }) do
  scenarios[#scenarios + 1] = { format = format, close = false }
  scenarios[#scenarios + 1] = { format = format, close = true }
end

local track

local function run(index)
  if track then reaper.DeleteTrack(track) end
  local scenario = scenarios[index]
  if not scenario then
    drive.pass("wheel bursts were recorded and released in both formats")
    return
  end
  local label = scenario.format .. (scenario.close and " closed" or " burst")
  reaper.InsertTrackAtIndex(0, false)
  track = reaper.GetTrack(0, 0)
  local fx = reaper.TrackFX_AddByName(track, scenario.format .. ":" .. NAME, false, -1)
  if fx < 0 then
    drive.fail(label, ": ", NAME, " did not load")
    return
  end
  local param, name = drive.param(track, fx)
  if not param then
    drive.error(label, ": no parameter ", os.getenv("PARAM") or "at all")
    return
  end
  local env = reaper.GetFXEnvelope(track, fx, param, true)
  local _, chunk = reaper.GetEnvelopeStateChunk(env, "", false)
  reaper.SetEnvelopeStateChunk(env, (chunk:gsub("\nARM %d", "\nARM 1")), false)
  reaper.DeleteEnvelopePointRange(env, -1, 1e9)
  reaper.InsertEnvelopePoint(env, 0, FLAT, 0, 0, false, true)
  reaper.InsertEnvelopePoint(env, 20, FLAT, 0, 0, false, true)
  reaper.Envelope_SortPoints(env)
  reaper.SetTrackAutomationMode(track, 2) -- touch
  reaper.TrackFX_SetParamNormalized(track, fx, param, FLAT)

  reaper.TrackFX_Show(track, fx, 3)
  reaper.SetEditCurPos(0, false, false)
  reaper.OnPlayButton()
  local started, moved, closed = reaper.time_precise(), false, false
  local function step()
    local value = reaper.TrackFX_GetParamNormalized(track, fx, param)
    if value > FLAT + 0.001 then moved = true end
    if scenario.close and moved and not closed then
      reaper.TrackFX_Show(track, fx, 2)
      closed = true
    end
    if reaper.time_precise() - started < PLAY_S then
      drive.defer(step)
      return
    end
    reaper.OnStopButton()
    local peak = FLAT
    for point = 0, reaper.CountEnvelopePoints(env) - 1 do
      local _, _, level = reaper.GetEnvelopePoint(env, point)
      peak = math.max(peak, level)
    end
    drive.say(string.format("%s: %s recorded up to %.3f, %.3f at the end", label, name, peak,
      value))
    if not moved then
      drive.error(label, ": no wheel notch arrived (is this an e2e-hooks build?)")
    elseif not scenario.close and peak < FLAT + NOTCHES * STEP - 0.011 then
      drive.fail(string.format("%s: %d notches recorded only up to %.3f, not %.3f", label,
        NOTCHES, peak, FLAT + NOTCHES * STEP))
    elseif value > FLAT + 0.001 then
      drive.fail(string.format("%s: %s stayed at %.3f instead of returning to the automation",
        label, name, value))
    else
      run(index + 1)
    end
  end
  drive.defer(step)
end

drive.start(function() run(1) end)
