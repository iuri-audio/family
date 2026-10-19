-- A check run by `cargo xtask e2e` (see ../../driving-reaper.md): in each format, with the editor
-- open, a parameter follows the host's automation through playback and settles once playback
-- stops. An editor that sends back stale values pulls it off the ramp. A plugin without an editor
-- runs it with none open. A format whose host won't automate the parameter, as CLAP Reaper won't
-- one the plugin marks not automatable, is left out; with none left, or no parameters, it skips.
--
-- Environment (set by `cargo xtask e2e`):
--   PLUGIN_NAME  the plugin's name, e.g. "Doubler"
--   PLUGIN_EDITOR  "1" if the plugin has an editor of its own, "0" if not
--   PARAM        optional: the parameter to exercise, as the host names it (a continuous one)

local drive = dofile(os.getenv("PRELUDE"))
local NAME = os.getenv("PLUGIN_NAME")
local EDITOR = os.getenv("PLUGIN_EDITOR") ~= "0"

local formats = { "VST3", "CLAP" }
-- The ramp: from FROM at 0 s to TO at RAMP_S.
local FROM, TO, RAMP_S = 0.2, 0.8, 4
-- Playback stops partway up the ramp.
local STOP_S = 3
-- How far the parameter may be off the ramp. Reaper processes FX ahead of the play position, so
-- on Windows it was 0.03 off (about 200 ms at the ramp's slope); a stale value sent back by the
-- editor is much further off.
local TOLERANCE = 0.1
-- The first moments of playback are left out, while the device starts.
local SKIP_S = 0.3

local track, fx, param, env
-- The formats that ran the ramp.
local automated = 0

local function value() return reaper.TrackFX_GetParamNormalized(track, fx, param) end

local check_format

local function next_format(index)
  if track then reaper.DeleteTrack(track) end
  local format = formats[index]
  if not format then
    if automated == 0 then
      drive.skip(NAME, " has no parameter the host will automate")
    else
      drive.pass(automated, " of ", #formats, " formats followed the automation",
        EDITOR and " with the editor open" or "")
    end
    return
  end
  reaper.InsertTrackAtIndex(0, false)
  track = reaper.GetTrack(0, 0)
  fx = reaper.TrackFX_AddByName(track, format .. ":" .. NAME, false, -1)
  if fx < 0 then
    drive.fail(format, ": ", NAME, " did not load")
    return
  end
  check_format(index, format)
end

function check_format(index, format)
  local name
  param, name = drive.param(track, fx)
  if not param then
    if os.getenv("PARAM") then
      drive.error(format, ": no parameter ", os.getenv("PARAM"))
    else
      drive.skip(NAME, " has no parameter to automate")
    end
    return
  end
  env = reaper.GetFXEnvelope(track, fx, param, true)
  if not env then
    drive.say(format, ": the host won't automate ", name, "; left out")
    next_format(index + 1)
    return
  end
  automated = automated + 1
  reaper.DeleteEnvelopePointRange(env, -1, 1e9)
  reaper.InsertEnvelopePoint(env, 0, FROM, 0, 0, false, true)
  reaper.InsertEnvelopePoint(env, RAMP_S, TO, 0, 0, false, true)
  reaper.Envelope_SortPoints(env)
  reaper.SetTrackAutomationMode(track, 1) -- read
  if EDITOR then reaper.TrackFX_Show(track, fx, 3) end

  drive.after(1.5, function()
    reaper.SetEditCurPos(0, false, false)
    reaper.OnPlayButton()
    local started, worst, worst_at, samples = reaper.time_precise(), 0, 0, 0
    local function sample()
      local elapsed = reaper.time_precise() - started
      if elapsed > 1 and reaper.GetPlayState() & 1 == 0 then
        drive.error(format, ": playback didn't start (is Reaper's audio device working?)")
        return
      end
      -- The position of the block the plugin is processing, which is what its value follows.
      local at = reaper.GetPlayPosition2()
      if at >= SKIP_S then
        local _, expected = reaper.Envelope_Evaluate(env, at, 0, 0)
        local off = math.abs(value() - expected)
        samples = samples + 1
        if off > worst then worst, worst_at = off, at end
      end
      if at < STOP_S then
        drive.defer(sample)
        return
      end
      reaper.OnStopButton()
      -- On stopping, Reaper may move the parameter to the automation at the edit cursor, so the
      -- check is that it settles, not where.
      local settled
      drive.after(0.3, function() settled = value() end)
      drive.after(1.3, function()
        drive.say(string.format("%s: %s followed the ramp within %.3f (worst at %.2f s, %d samples)",
          format, name, worst, worst_at, samples))
        if samples < 10 then
          drive.error(format, ": only ", samples, " samples during playback")
        elseif worst > TOLERANCE then
          drive.fail(string.format("%s: %s was %.3f off the automation at %.2f s", format, name,
            worst, worst_at))
        elseif math.abs(value() - settled) > 0.001 then
          drive.fail(string.format("%s: %s was %.3f after playback stopped, %.3f a second later",
            format, name, settled, value()))
        else
          reaper.TrackFX_Show(track, fx, 2)
          next_format(index + 1)
        end
      end)
    end
    drive.defer(sample)
  end)
end

drive.start(function() next_format(1) end)
