-- A page's `random` (page_random; 03 §4.5) is a function of the page, not a draw (0047 A29):
-- the first 52 bits of the MD5 of `{tenant}:{page_id}`, as a fraction in [0, 1), the same
-- expression the projections write (`page_random_sql!` in triplespace-projections), so that
-- a rebuild projects what the write path did. Idempotent: it recomputes every row.
UPDATE view.page
   SET random = (('x' || substr(md5(tenant || ':' || page_id::text), 1, 13))::bit(52)::bigint)::double precision
                / 4503599627370496.0;
