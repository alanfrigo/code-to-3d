use serde::{Deserialize, Serialize};
use std::fmt::Debug;

type Point = [f64; 3];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    position_tolerance_m: f64,
    time_tolerance_seconds: f64,
    fixture: Fixture,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    schema_version: u32,
    mesh: Mesh,
    bones: Vec<Bone>,
    socket_bone: String,
    clip: Clip,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Mesh {
    name: String,
    has_skin: bool,
    aabb_min_m: Point,
    aabb_max_m: Point,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct Bone {
    name: String,
    parent: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Clip {
    name: String,
    duration_seconds: f64,
    samples: Vec<Sample>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    time_seconds: f64,
    root_m: Point,
    hinge_m: Point,
    socket_m: Point,
}

pub fn compare(profile: &Profile, observed: &Fixture) -> Vec<String> {
    let expected = &profile.fixture;
    let mut checks = Checks {
        profile,
        failures: Vec::new(),
    };
    checks.equal(
        "schema_version",
        &expected.schema_version,
        &observed.schema_version,
    );
    let (want, got) = (&expected.mesh, &observed.mesh);
    checks.equal("mesh.name", &want.name, &got.name);
    checks.equal("mesh.has_skin", &want.has_skin, &got.has_skin);
    checks.point("mesh.aabb_min_m", want.aabb_min_m, got.aabb_min_m);
    checks.point("mesh.aabb_max_m", want.aabb_max_m, got.aabb_max_m);
    checks.equal("bones", &expected.bones, &observed.bones);
    checks.equal("socket_bone", &expected.socket_bone, &observed.socket_bone);
    let (want, got) = (&expected.clip, &observed.clip);
    checks.equal("clip.name", &want.name, &got.name);
    checks.time(
        "clip.duration_seconds",
        want.duration_seconds,
        got.duration_seconds,
    );
    checks.equal("clip.samples.len", &want.samples.len(), &got.samples.len());
    for (index, (want, got)) in want.samples.iter().zip(&got.samples).enumerate() {
        checks.time(
            format_args!("clip.samples[{index}].time_seconds"),
            want.time_seconds,
            got.time_seconds,
        );
        for (field, want, got) in [
            ("root_m", want.root_m, got.root_m),
            ("hinge_m", want.hinge_m, got.hinge_m),
            ("socket_m", want.socket_m, got.socket_m),
        ] {
            checks.point(format_args!("clip.samples[{index}].{field}"), want, got);
        }
    }
    checks.failures
}

struct Checks<'a> {
    profile: &'a Profile,
    failures: Vec<String>,
}

impl Checks<'_> {
    fn equal<T: PartialEq + Debug>(&mut self, field: &str, expected: &T, observed: &T) {
        if expected != observed {
            self.failures.push(format!(
                "{field}: expected {expected:?}, observed {observed:?}"
            ));
        }
    }

    fn time(&mut self, field: impl std::fmt::Display, expected: f64, observed: f64) {
        let tolerance = self.profile.time_tolerance_seconds;
        if (expected - observed).abs() > tolerance {
            self.failures.push(format!(
                "{field}: expected {expected} s, observed {observed} s (tolerance {tolerance} s)"
            ));
        }
    }

    fn point(&mut self, field: impl std::fmt::Display, expected: Point, observed: Point) {
        let tolerance = self.profile.position_tolerance_m;
        let [x, y, z] = std::array::from_fn::<_, 3, _>(|i| expected[i] - observed[i]);
        if x * x + y * y + z * z > tolerance * tolerance {
            self.failures.push(format!(
                "{field}: expected {expected:?} m, observed {observed:?} m (tolerance {tolerance} m)"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_pose_regression_between_clip_endpoints() {
        let profile: Profile = serde_json::from_str(crate::PROFILE).unwrap();
        let fixture_json = serde_json::to_string(&profile.fixture).unwrap();
        let mut observed: Fixture = serde_json::from_str(&fixture_json).unwrap();
        observed.clip.samples[1].socket_m[0] -= 0.3;
        let failures = compare(&profile, &observed);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("clip.samples[1].socket_m:"));
    }
}
