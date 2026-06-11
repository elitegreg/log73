use radio_cat_rs::Frequency;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub meters: u16,
    pub name: &'static str,
    pub lower: Frequency,
    pub upper: Frequency,
}

pub const USA_AMATEUR_BANDS: &[Band] = &[
    Band {
        meters: 160,
        name: "160m",
        lower: Frequency::from_khz(1_800),
        upper: Frequency::from_khz(2_000),
    },
    Band {
        meters: 80,
        name: "80m",
        lower: Frequency::from_khz(3_500),
        upper: Frequency::from_khz(4_000),
    },
    Band {
        meters: 60,
        name: "60m",
        lower: Frequency::from_hz(5_330_500),
        upper: Frequency::from_hz(5_406_500),
    },
    Band {
        meters: 40,
        name: "40m",
        lower: Frequency::from_khz(7_000),
        upper: Frequency::from_khz(7_300),
    },
    Band {
        meters: 30,
        name: "30m",
        lower: Frequency::from_khz(10_100),
        upper: Frequency::from_khz(10_150),
    },
    Band {
        meters: 20,
        name: "20m",
        lower: Frequency::from_khz(14_000),
        upper: Frequency::from_khz(14_350),
    },
    Band {
        meters: 17,
        name: "17m",
        lower: Frequency::from_khz(18_068),
        upper: Frequency::from_khz(18_168),
    },
    Band {
        meters: 15,
        name: "15m",
        lower: Frequency::from_khz(21_000),
        upper: Frequency::from_khz(21_450),
    },
    Band {
        meters: 12,
        name: "12m",
        lower: Frequency::from_khz(24_890),
        upper: Frequency::from_khz(24_990),
    },
    Band {
        meters: 10,
        name: "10m",
        lower: Frequency::from_khz(28_000),
        upper: Frequency::from_khz(29_700),
    },
    Band {
        meters: 6,
        name: "6m",
        lower: Frequency::from_khz(50_000),
        upper: Frequency::from_khz(54_000),
    },
    Band {
        meters: 2,
        name: "2m",
        lower: Frequency::from_khz(144_000),
        upper: Frequency::from_khz(148_000),
    },
];

pub fn band_for_frequency(frequency: Frequency) -> Option<&'static Band> {
    USA_AMATEUR_BANDS
        .iter()
        .find(|band| frequency >= band.lower && frequency <= band.upper)
}
