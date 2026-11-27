"""Append the original finish-owner near decisions with byte-audited bodies."""
import hashlib
import json
from pathlib import Path


def append_finish_controls(root: Path, output: Path, game: Path):
    relative = 'vehicle_runtime/opponents.rs'
    original = root / 'crates/game/src' / relative
    raw = original.read_bytes()
    text = raw.decode()
    method = text.index('    pub(super) fn update_opponent(')
    horn = text.index('                car.horn_down = false;', method)
    start = text.index('                let a = car.ai.as_mut().ok_or(-1)?;', horn)
    end = text.index('                if !rules.finish_owner', start)
    decision = text[start:end]
    start_tail = text.index('                car.input.drive.throttle = car.input.drive.throttle.clamp(', end)
    end_tail = text.index('                routes::spacing(car, &config, &global, rules.difficulty)?;', start_tail)
    end_tail += len('                routes::spacing(car, &config, &global, rules.difficulty)?;')
    tail = text[start_tail:end_tail]
    for anchor in ['driving::throttle(', 'driving::steer(', 'driving::reverse(', 'a.turn_time > 50', 'a.confusion.wrapping_sub']:
        if decision.count(anchor) != 1:
            raise ValueError('finish decision source changed: ' + anchor)
    if tail.count('crate::random::next(') != 1 or tail.count('routes::spacing(') != 1:
        raise ValueError('finish control tail source changed')
    # Only borrow construction and the finish-owner's false combat guard are
    # outside this extracted stage. Decision and publication statements are exact.
    suffix = '''
struct RccFinishFrame { dt: i32 }
struct RccFinishShared { random: u32 }
struct RccFinishControls { frame: RccFinishFrame, shared: RccFinishShared }
impl RccFinishControls {
    fn step(&mut self, car: &mut crate::vehicle_startup::Player,
        routes: &crate::waypoint::View<'_>, config: tankracer_assets::config::Ai,
        global: tankracer_assets::object_catalog::Opponents,
        rules: crate::ai::driving::Rules) -> Result<(),i32> {
        car.ai.as_ref().ok_or(-1)?;
        car.horn_down = false;
''' + decision + tail + '''
        Ok(())
    }
}
pub fn rcc_finish_controls(car: &mut crate::vehicle_startup::Player,
    routes: &crate::waypoint::View<'_>, config: tankracer_assets::config::Ai,
    global: tankracer_assets::object_catalog::Opponents, rules: crate::ai::driving::Rules,
    dt: i32, random: &mut u32) -> Result<(),i32> {
    assert!(rules.finish_owner && rules.difficulty < 6);
    let mut state=RccFinishControls { frame:RccFinishFrame { dt }, shared:RccFinishShared { random:*random } };
    let result=state.step(car,routes,config,global,rules);
    *random=state.shared.random;
    result
}
'''
    manifest_path = output / 'reference-overlay.json'
    manifest = json.loads(manifest_path.read_text())
    for name, extra in [(relative, suffix), ('vehicle_runtime.rs', '\npub use opponents::rcc_finish_controls;\n')]:
        source = root / 'crates/game/src' / name
        entry = next(e for e in manifest['sources'] if Path(e['file']) == source)
        target = game / 'src' / name
        expected = source.read_bytes() + entry['appended'].encode()
        if target.read_bytes() != expected:
            raise ValueError('finish reference overlay changed: ' + name)
        entry['appended'] += extra
        target.write_bytes(expected + extra.encode())
    manifest['finish_controls'] = dict(source=str(original), source_sha256=hashlib.sha256(raw).hexdigest(),
        decision_sha256=hashlib.sha256(decision.encode()).hexdigest(), tail_sha256=hashlib.sha256(tail.encode()).hexdigest(),
        scope='Original near finish-owner decisions after Tank and before waypoint/rank publication; no full Tick claim')
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
