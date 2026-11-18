use super::*;

pub(super) const A_HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const A_BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B_BASE: &str = "dddddddddddddddddddddddddddddddddddddddd";
const B_HEAD: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const MERGED_OID: &str = "cccccccccccccccccccccccccccccccccccccccc";

pub(super) fn applies(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::MergePrepare | Phase::MergeSend | Phase::MergeWrongHead
    )
}

pub(super) fn graphql(phase: Phase, payload: &Value) -> Option<Route> {
    if !applies(phase) {
        return None;
    }
    let query = payload.get("query")?.as_str()?;
    match payload.get("variables")? {
        id if *id == json!({"id":"PR_42"}) && query.starts_with("query ThreadlineMergePolicy(") => {
            Some(Route::MergePolicyA)
        }
        id if *id == json!({"id":"PR_43"}) && query.starts_with("query ThreadlineMergePolicy(") => {
            Some(Route::MergePolicyB)
        }
        id if *id == json!({"id":"PR_42"}) && query.starts_with("query ThreadlineMergedPull(") => {
            Some(Route::MergeReadback)
        }
        _ => None,
    }
}

pub(super) fn put(phase: Phase, path: &str, payload: &Value) -> Route {
    if phase == Phase::MergeSend
        && path == "/repos/fixture-org/private/pulls/42/merge"
        && *payload == json!({"sha": A_HEAD, "merge_method": "squash"})
    {
        Route::MergePut
    } else {
        Route::Unexpected
    }
}

pub(super) fn response(phase: Phase, route: Route, sent: bool) -> Option<Response> {
    if !applies(phase) {
        return None;
    }
    let healthy = |body| Response {
        status: 200,
        reason: "OK",
        body,
        sso: None,
    };
    if route == Route::PullA && sent {
        let mut response = pr::response(phase, route)?;
        response.body["state"] = json!("closed");
        response.body["merged"] = json!(true);
        response.body["merged_by"] = json!({"id":41,"login":"fixture-user"});
        return Some(response);
    }
    match route {
        Route::MergePut if !sent => Some(healthy(json!({"merged":true,"sha":MERGED_OID}))),
        Route::MergeReadback if sent => Some(healthy(json!({"data":{"node":{
            "id":"PR_42","number":42,"baseRefName":"main","baseRefOid":A_BASE,"headRefOid":A_HEAD,
            "repository":{"id":"R_71"},"state":"MERGED","merged":true,"mergeCommit":{"oid":MERGED_OID},
            "mergedBy":{"__typename":"User","databaseId":41}
        }}}))),
        Route::MergePolicyA | Route::MergePolicyB if phase != Phase::MergeWrongHead => {
            let (id, number, base, head) = if route == Route::MergePolicyA {
                ("PR_42", 42, A_BASE, A_HEAD)
            } else {
                ("PR_43", 43, B_BASE, B_HEAD)
            };
            Some(healthy(json!({"data":{"node":{
                "id":id,"number":number,"baseRefName":"main","baseRefOid":base,"headRefOid":head,
                "state":"OPEN","isDraft":false,"merged":false,"mergeable":"MERGEABLE",
                "mergeStateStatus":"CLEAN","reviewDecision":null,"mergeQueue":null,
                "repository":{"id":"R_71","viewerPermission":"WRITE","mergeCommitAllowed":true,
                    "squashMergeAllowed":true,"rebaseMergeAllowed":false}
            }}})))
        }
        _ => None,
    }
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let successful = |phase, route| {
        records
            .iter()
            .filter(|row| row.phase == phase && row.route == route && row.status == 200)
            .count()
    };
    let accepted = records.iter().position(|row| row.route == Route::MergePut);
    let readback = records
        .iter()
        .position(|row| row.route == Route::MergeReadback);
    records
        .iter()
        .all(|row| row.fixture_authorization && row.route != Route::Unexpected && row.status == 200)
        && successful(Phase::MergePrepare, Route::MergePolicyA) >= 1
        && successful(Phase::MergePrepare, Route::MergePolicyB) >= 1
        && successful(Phase::MergeSend, Route::MergePut) == 1
        && successful(Phase::MergeSend, Route::MergeReadback) >= 1
        && accepted
            .zip(readback)
            .is_some_and(|(accepted, readback)| accepted < readback)
        && successful(Phase::MergeWrongHead, Route::PullB) >= 1
        && successful(Phase::MergeWrongHead, Route::MergePut) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_only_merges_the_exact_squash_head_and_reads_back_the_same_pr() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        let request = |method, path, body: &str| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(socket, "{method} {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        let mutation = "/repos/fixture-org/private/pulls/42/merge";
        let pr = "/repositories/71/pulls/42";
        let send = json!({"sha":A_HEAD,"merge_method":"squash"}).to_string();
        let wrong = json!({"sha":B_HEAD,"merge_method":"squash"}).to_string();
        let proof = json!({"query":"query ThreadlineMergedPull($id:ID!) { node(id:$id) { id } }", "variables":{"id":"PR_42"}}).to_string();
        fs::write(fixture.phase_file(), "merge-prepare").unwrap();
        assert!(request("GET", pr, "").contains("\"merged\":false"));
        assert!(request("PUT", mutation, &send).starts_with("HTTP/1.1 404"));
        fs::write(fixture.phase_file(), "merge-send").unwrap();
        assert!(request("PUT", mutation, &wrong).starts_with("HTTP/1.1 404"));
        assert!(request("POST", "/graphql", &proof).starts_with("HTTP/1.1 404"));
        let accepted = request("PUT", mutation, &send);
        assert!(accepted.starts_with("HTTP/1.1 200"));
        assert!(accepted.contains("X-GitHub-Request-Id: SYNTHETIC:4201:09"));
        assert!(request("GET", pr, "").contains("\"merged\":true"));
        assert!(request("POST", "/graphql", &proof).contains(MERGED_OID));
        assert!(request("PUT", mutation, &send).starts_with("HTTP/1.1 404"));
        fixture.finish(temp.path(), TraceCheck::Observe).unwrap();
    }
}
