pub(crate) fn wrapper_fixture() {
    wrapper_inner();
}

fn wrapper_inner() {
    crate::features::command_facade::entrypoint::start();
}
