# Earth-space family synthesis

## Family verdict

Earth-space practitioners win when one typed case preserves spatial or physical meaning from discovery and standard files through labeled arrays, geometry or orbit dynamics, execution, visualization, and a reproducible result. The seven-domain census shows useful Jet substrate—typed data and tables, Tensor/linalg/FFT, units, files and bytes, HTTP, bounded streams, notebook cells, deterministic plots, authority, jobs and evidence—but domain models are mostly real gaps. The strongest shared cuts are labeled arrays and CF metadata, geometry/CRS and raster grids, chunked lazy plans, scientific formats, catalog queries, reference frames, mesh/stencils, distributed execution, and evidence. Domain-specific orbit, seismic, ocean, astronomy and provider layers must attach to those mechanisms without pretending generic primitives are domain support.

The matrix contains 31 shared mechanisms: 9 new Earth-space candidates and 22 reused IDs from the engineering family. Reused IDs are intentionally not renamed or forked.

## Mechanisms

| ID | Mechanism | Domains | Gate kind | Jet state | Identity |
|---|---|---:|---|---|---|
| M-LINALG | Typed dense tensors and linear algebra | 2 | none | mixed | reuse M-LINALG |
| M-FFT | FFT and inverse spectral transforms | 2 | public-api | mixed | reuse M-FFT |
| M-SOLVER-SUBSTRATE | Checked nonlinear and domain solver substrate | 2 | public-api | mixed | reuse M-SOLVER-SUBSTRATE |
| M-SPARSE-SOLVERS | Sparse assembly, direct factorization, and Krylov solves | 1 | public-api | real-gap | reuse M-SPARSE-SOLVERS |
| M-ODE-DAE | ODE and DAE integration with events | 1 | public-api | real-gap | reuse M-ODE-DAE |
| M-OPTIMIZATION | Constrained optimization and design studies | 2 | public-api | real-gap | reuse M-OPTIMIZATION |
| M-UNITS | Typed dimensional and affine quantities | 3 | invariant | mixed | reuse M-UNITS |
| M-MPI-LAUNCH | MPI launch and distributed domain execution | 2 | stdlib-dependency | real-gap | reuse M-MPI-LAUNCH |
| M-NOTEBOOK | Notebook and interactive REPL loop | 2 | none | mixed | reuse M-NOTEBOOK |
| M-PLOTTING | Deterministic and interactive engineering visualization | 1 | ui | mixed | reuse M-PLOTTING |
| M-SCI-FORMATS | Scientific and engineering file formats | 7 | public-api | mixed | reuse M-SCI-FORMATS |
| M-DSP-FILTERS | Typed filter design and multirate DSP | 2 | public-api | real-gap | reuse M-DSP-FILTERS |
| M-REALTIME-STREAMS | Bounded real-time audio and device streams | 1 | invariant | mixed | reuse M-REALTIME-STREAMS |
| M-MESH-STENCILS | Meshes, stencils, and discretized field operators | 3 | public-api | real-gap | reuse M-MESH-STENCILS |
| M-DATA-TABLES | Typed tables, records, and result schemas | 4 | none | mixed | reuse M-DATA-TABLES |
| M-WORKFLOW-DAG | Inspectable workflow graphs and execution plans | 4 | public-api | mixed | reuse M-WORKFLOW-DAG |
| M-SIMULATION | Reproducible domain simulation lifecycle | 5 | public-api | mixed | reuse M-SIMULATION |
| M-EVIDENCE | Inspection, diagnostics, and reproducible evidence | 4 | none | mixed | reuse M-EVIDENCE |
| M-NETWORKING | Typed network and service protocol boundaries | 3 | none | mixed | reuse M-NETWORKING |
| M-PACKAGING | Package, dependency, and native-provider boundary | 1 | stdlib-dependency | mixed | reuse M-PACKAGING |
| M-STORAGE | Durable scientific stores and caches | 3 | stdlib-dependency | mixed | reuse M-STORAGE |
| M-HARDWARE-IO | Authority-safe hardware and instrument I/O | 1 | public-api | real-gap | reuse M-HARDWARE-IO |
| M-LABELED-ARRAYS | Labeled N-D arrays, coordinates, and CF metadata | 4 | public-api | real-gap | new |
| M-GEOMETRY-CRS | Geometry, CRS, geodesics, and spatial validity | 3 | public-api | real-gap | new |
| M-SPATIAL-INDEX | Spatial indexes, spherical cells, and indexed joins | 2 | public-api | real-gap | new |
| M-RASTER-GRIDS | Georeferenced raster grids, masks, and reprojection | 4 | public-api | real-gap | new |
| M-CHUNKED-LAZY-ARRAYS | Chunked lazy arrays and overlap-aware plans | 2 | public-api | real-gap | new |
| M-REFERENCE-FRAMES | Astronomical and mission time scales and reference frames | 2 | invariant | real-gap | new |
| M-ORBIT-DYNAMICS | Celestial and mission dynamics: orbit states, propagation, force models, and events | 2 | public-api | real-gap | new |
| M-CATALOG-ARCHIVES | Typed archive, catalog, and asset queries | 4 | public-api | real-gap | new |
| M-MAP-RENDERING | Map, raster, tile, and scenario rendering | 3 | ui | real-gap | new |

Each mechanism p0_rows quotes the exact census feature and need for its domain. Domain-only details remain in domains.json.

The broadest mechanisms are M-SCI-FORMATS (7 domains), then the four-domain tie among M-DATA-TABLES, M-WORKFLOW-DAG, M-SIMULATION, M-EVIDENCE, M-LABELED-ARRAYS and M-CATALOG-ARCHIVES.

## Domain matrix

| Domain | Benchmark peer | P0 rows | Performance incumbent | Gauntlet | Single biggest beat vector |
|---|---|---:|---|---|---|
| gis-geospatial | PostGIS/GiST + GEOS; GDAL; H3 | 25 | PostGIS with GiST/GEOS for indexed spatial joins, GDAL for raster reprojection and vector/tile conversion, and H3 for hierarchical global indexing; QGIS, GeoPandas/Shapely, MapLibre, and deck.gl are the workflow and rendering peers. | none (no GIS/geospatial entry found in gauntlet/matrix.json, gauntlet/entries/, or gauntlet/measurement-manifest.json) | Typed failure over silent raster coercion can name geometry, CRS, nodata and provider faults. |
| remote-sensing | Dask+xarray+Rasterio/GDAL | 32 | Dask Array + xarray.open_mfdataset + Rasterio/GDAL block IO | none | Typed failure over silent raster coercion can preserve masks, CRS and finite-value diagnostics. |
| climate-weather | WRF v4.4 Fortran ARW dmpar MPI; xarray+Dask+Zarr+xESMF | 25 | WRF v4.4 ARW real-data benchmark | none | Typed physical data can make axes, CF metadata, units, calendars, CRS and source identity one ledger. |
| geoscience-seismology | Madagascar sfsrmig3 + SeisComP | 24 | Madagascar sfsrmig3 (migration) and SeisComP scautopick/scautoloc (real-time picking/location) | none | One typed evidence ledger can join file/live input, samples, metadata, compute, replay and review. |
| astronomy-astrophysics | Gadget-4 TreePM/FMM + Enzo AMR; Astropy/NumPy; CASA | 28 | Gadget-4 C++11 with MPI and TreePM/FMM for cosmological N-body/SPH; Enzo for adaptive-mesh multiphysics; Astropy/NumPy/SciPy/healpy C/C++ kernels for FITS, catalogs, WCS, HEALPix, and spectra; CASA for radio imaging. | numerics.float-kernel (nbody proxy); no dedicated FITS/WCS/catalog, HEALPix, CASA, Gadget, Enzo, or astronomy notebook cell | One typed scientific value ledger can reject unit, uncertainty, shape and metadata mistakes before long runs. |
| oceanography-hydrology | MODFLOW 6.7 + FloPy; ROMS 3.6 | 22 | MODFLOW 6.7.0 GWF for groundwater flow, with ROMS v3.6 as the ocean-step incumbent | none — gauntlet/matrix.json:13-60 contains generic text, formats, numerics, files, time, concurrency, CLI, web, network, and embedded cells but no oceanography-hydrology, MODFLOW, ROMS, MOM6, HEC-RAS, or SWMM cell. | Typed setup and evidence can reject incompatible dimensions, shapes, ownership and defaults before a long run. |
| space-satellite | Orekit Java; GMAT C++; poliastro | 23 | Orekit Java numerical propagation and GMAT C++ mission-analysis propagation; poliastro Numba is the closest scriptable Python comparator | none (nearest generic cell is numerics.float-kernel; gauntlet/matrix.json has no orbit, contact-scheduling, CCSDS, or telemetry cell) | Typed quantities and diagnostics can reject length/time/frame/unit mistakes before numerical execution. |

The performance objects preserve each miner's incumbent, workload, dataset, published numbers, causal speed explanation and required Jet proof. No domain has a dedicated earth-space gauntlet cell; astronomy only points to the generic numerics.float-kernel n-body proxy. Published competitor timings are leads, not Jet wins.

## Family owner gates

| Suggested decision | Gate | Affected mechanisms/domains |
|---|---|---|
| D-ES-ARRAYS1 | Ratify labeled N-D arrays, coordinate identity, CF metadata, calendars, masks, alignment, lazy chunks and all-tier parity. | M-LABELED-ARRAYS, M-CHUNKED-LAZY-ARRAYS; remote, climate, ocean, astronomy |
| D-ES-GEOMETRY1 | Ratify geometry, CRS/SRID, geodesics, raster grids, nodata, reprojection, spatial indexes and map/tile metadata. | M-GEOMETRY-CRS, M-SPATIAL-INDEX, M-RASTER-GRIDS, M-MAP-RENDERING; GIS, remote, climate, ocean, astronomy, space |
| D-ES-FORMATS1 | Choose first-party versus audited bridges for GeoJSON/GeoPackage/MVT, netCDF/CF/Zarr, FITS/HDF5/VOTable/MeasurementSet, waveform formats and CCSDS messages. | M-SCI-FORMATS, M-STORAGE, M-CATALOG-ARCHIVES; all seven domains |
| D-ES-FRAMES1 | Ratify time scales, calendars, CRS, WCS, Earth orientation, reference-frame registries and provenance. | M-REFERENCE-FRAMES, M-UNITS, M-GEOMETRY-CRS; GIS, climate, astronomy, ocean, space |
| D-ES-EXEC1 | Choose chunk schedulers, MPI launch/halos, GPU/provider boundaries, cancellation, retry and resource receipts. | M-CHUNKED-LAZY-ARRAYS, M-MPI-LAUNCH, M-MESH-STENCILS, M-PACKAGING; remote, climate, astronomy, ocean, GIS |
| D-ES-DYNAMICS1 | Choose orbit, seismic, climate, ocean and astronomy model ownership, solver/integrator results, event/convergence semantics and native dependencies. | M-ODE-DAE, M-SOLVER-SUBSTRATE, M-SPARSE-SOLVERS, M-SIMULATION, M-ORBIT-DYNAMICS; climate, geoscience, astronomy, ocean, space |
| D-ES-IO1 | Ratify FDSN/SeedLink, telemetry, packet, HTTP/catalog authority, deadlines, backpressure, cache and safety policy. | M-REALTIME-STREAMS, M-HARDWARE-IO, M-NETWORKING, M-CATALOG-ARCHIVES; geoscience, remote, astronomy, space |
| D-ES-EVIDENCE1 | Require fixed public fixtures, exact output hashes, cross-tier parity, provenance and strict level-playing-field gauntlets before speed claims. | M-EVIDENCE, M-PLOTTING, M-WORKFLOW-DAG; all seven domains |
| D-ES-DX1 | Extend existing notebook, live loop, web rendering and package boundaries rather than creating domain evaluators. | M-NOTEBOOK, M-PLOTTING, M-PACKAGING; notebook and publication domains |

## Contradictions and defects

| Mechanism | Source 1 | Source 2 | Synthesis |
|---|---|---|---|
| M-CHUNKED-LAZY-ARRAYS | remote-sensing/report.md §Performance calls Dask+xarray+Rasterio the scale path and records a 200us–1ms task-overhead lead. | climate-weather/report.md §Jet today says a narrow jet run data probe succeeds, while default TIR rejects lazy/readers/joins at data_calls.rs:461-466. | Treat the data probe as a substrate smoke only; no lazy all-tier parity or chunk scheduler is shipped, and no Dask number is a Jet comparison. |
| M-SCI-FORMATS | remote-sensing/report.md §Feature census says generic bytes and encoding do not provide GeoTIFF/Zarr/STAC product semantics. | climate-weather and oceanography-hydrology reports describe netCDF/CF/Zarr interchange, while astronomy and space require FITS/HDF5/CCSDS schemas. | Format support means typed schema, metadata, round trip and loss policy per family, not generic JSON/bytes decoding. |
| M-REFERENCE-FRAMES | astronomy-astrophysics/report.md §Beat vectors requires Time, SkyCoord, IERS and WCS semantics. | space-satellite/report.md §Owner gates requires mission time scales, EOP, frame hierarchy and element-kind distinctions, including TLE/OMM warnings. | Share explicit time/frame identity and provenance, but keep terrestrial, celestial and mission frame registries distinct where transforms differ. |
| M-LINALG / M-ODE-DAE | astronomy-astrophysics/performance.json records an n-body proxy loss: Jet AOT 0.1102899s vs Rust 0.00488395s and default 24.9266s. | space-satellite/performance.json records published Orekit/GMAT/poliastro values but gauntlet_cell none; ocean and climate also have no domain cells. | No cross-domain speed claim is established. Build same-input cells first; generic math or published incumbent values are not domain wins. |
| Generic substrate versus domain model | gis-geospatial/report.md §Jet today has Tensor, units and typed tables but direct core.geo reports E1001. | space-satellite and geoscience-seismology reports likewise say generic Tensor/FFT/streams do not provide orbit or seismic models. | Reuse substrate mechanisms, but require domain types, metadata, errors and fixtures before counting support. |

Known actionable probe defects are listed in defects.json; expected negative-path E1001 captures remain evidence of absent modules, not compiler regressions.

## Sources and limits

Inputs were the seven registry rows, each domain census.json, performance.json, report.md and claims.json, the prior engineering mechanism file, dx DESIGN.md and the e14 developer-experience proposal. No Tower writes, builds or project-wide validation were run.
