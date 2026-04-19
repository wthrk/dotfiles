{
  homebrew = {
    enable = true;
    taps = [ ];

    onActivation = {
      autoUpdate = false;
      upgrade = false;
      cleanup = "none";
    };

    casks = [
      "font-cica"
    ];

    masApps = { };
  };
}
