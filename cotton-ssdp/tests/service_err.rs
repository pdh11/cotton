use cotton_ssdp::*;

#[test]
#[cfg_attr(miri, ignore)]
fn getinterface_failure_passed_on() {
    const SSDP_TOKEN1: mio::Token = mio::Token(1);
    const SSDP_TOKEN2: mio::Token = mio::Token(2);
    let poll = mio::Poll::new().unwrap();

    // Setting a low limit of open sockets is one of the few ways to cause
    // cotton_netif::get_interfaces to fail (and to cover that code path)
    let mut lim = libc::rlimit {
        rlim_cur: 0u64,
        rlim_max: 10_000u64,
    };
    unsafe {
        libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim);
        let mut new_lim = lim.clone();
        new_lim.rlim_cur = 0;
        libc::setrlimit(libc::RLIMIT_NOFILE, &new_lim);
    }

    let rc = Service::new(poll.registry(), (SSDP_TOKEN1, SSDP_TOKEN2));

    // Restore a decent limit again or cargo test itself gets in a pickle!
    unsafe {
        libc::setrlimit(libc::RLIMIT_NOFILE, &lim);
    }

    assert!(rc.is_err());
}
