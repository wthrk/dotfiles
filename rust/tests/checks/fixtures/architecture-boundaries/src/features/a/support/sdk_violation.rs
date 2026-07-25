use git2::Repository;

pub(crate) fn support_sdk_fixture() {
    let _ = Repository::open;
}
