use super::route_detail;

#[test]
fn route_details_name_the_method_a_verb_states() {
    assert_eq!(route_detail("get", Some("/a")), "GET /a");
    assert_eq!(route_detail("GetMapping", Some("/a")), "GET /a");
    assert_eq!(route_detail("HttpDelete", None), "DELETE");
    assert_eq!(route_detail("MapPost", Some("/a")), "POST /a");
    assert_eq!(route_detail("RequestMapping", Some("/a")), "ANY /a");
    assert_eq!(route_detail("HandleFunc", Some("/a")), "ANY /a");
}
