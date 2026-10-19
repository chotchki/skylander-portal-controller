-- The 12 variant ids AA's interim catalogue keyed by the product's 0x0BFF
-- mask, rewritten to the ids the disc roster gives them (openspec AE.4, design
-- decision 2): Giants' own subtype word, bit 10 kept. A figure a profile used
-- or left on the portal under the old id comes back as the same figure. Every
-- old id is unique by toy type and none is a disc id, so an exact-string
-- rewrite can't touch another figure; `figures::AA_IDS_ON_THE_DISC` holds the
-- same pairs for the tests.

UPDATE figure_usage SET figure_id = CASE figure_id
    WHEN '000000-0802' THEN '000000-0402' -- Polar Whirlwind
    WHEN '000012-0802' THEN '000012-0402' -- Royal Double Trouble
    WHEN '00000a-0803' THEN '00000a-0403' -- Legendary Ignitor
    WHEN '00000f-0803' THEN '00000f-0403' -- Legendary Slam Bam
    WHEN '00001a-0803' THEN '00001a-0403' -- Legendary Stealth Elf
    WHEN '000066-0202' THEN '000066-0402' -- Granite Crusher
    WHEN '00006d-0202' THEN '00006d-0402' -- Scarlet Ninjini
    WHEN '000070-0202' THEN '000070-0402' -- Gnarly Tree Rex
    WHEN '000069-0002' THEN '000069-0402' -- Molten Hot Dog
    WHEN '00006c-0002' THEN '00006c-0402' -- Punch Pop Fizz
    WHEN '00006a-0203' THEN '00006a-0603' -- Legendary Chill
    WHEN '00006e-0203' THEN '00006e-0403' -- Legendary Bouncer
    ELSE figure_id END
WHERE figure_id IN (
    '000000-0802',
    '000012-0802',
    '00000a-0803',
    '00000f-0803',
    '00001a-0803',
    '000066-0202',
    '00006d-0202',
    '000070-0202',
    '000069-0002',
    '00006c-0002',
    '00006a-0203',
    '00006e-0203'
);

-- The saved layout is a JSON array of slots, each id in quotes.
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"000000-0802"', '"000000-0402"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"000012-0802"', '"000012-0402"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"00000a-0803"', '"00000a-0403"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"00000f-0803"', '"00000f-0403"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"00001a-0803"', '"00001a-0403"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"000066-0202"', '"000066-0402"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"00006d-0202"', '"00006d-0402"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"000070-0202"', '"000070-0402"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"000069-0002"', '"000069-0402"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"00006c-0002"', '"00006c-0402"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"00006a-0203"', '"00006a-0603"');
UPDATE sessions SET last_portal_layout_json =
    REPLACE(last_portal_layout_json, '"00006e-0203"', '"00006e-0403"');
