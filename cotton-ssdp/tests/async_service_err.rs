use cotton_ssdp::AsyncService;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn getinterface_failure_passed_on() {
    // Setting a low limit of open sockets is one of the few ways to cause
    // cotton_netif::get_interfaces_async to fail (and to cover that code path)
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 10_000,
    };
    unsafe {
        libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim);
        let mut new_lim = lim;
        new_lim.rlim_cur = 0;
        libc::setrlimit(libc::RLIMIT_NOFILE, &new_lim);
    }

    let rc = AsyncService::new();

    // Restore a decent limit again or cargo test itself gets in a pickle!
    unsafe {
        libc::setrlimit(libc::RLIMIT_NOFILE, &lim);
    }

    assert!(rc.is_err());
}
