exports.nameA = "a";
// Goes to cyc-b and back: cyc-a and cyc-b depend on each other
exports.viaB = () => `${require("@tkfixture/cyc-b").nameB}>${require("@tkfixture/cyc-b").backToA()}`;
