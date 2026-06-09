{ lib, nirionConfig }:

let
  imageTransform =
    projectName: serviceName: serviceConfig:
    if builtins.hasAttr "image" serviceConfig then
      let
        lockedImages = nirionConfig.out.locked_images;
        lockedImage = lockedImages."${projectName}.${serviceName}" or null;
      in
      serviceConfig // { image = lockedImage; }
    else
      serviceConfig;

  lockedImageTransform =
    projectName: serviceName: serviceConfig:
    if builtins.hasAttr "locked_image" serviceConfig then
      let
        lockedImages = nirionConfig.out.locked_images;
        lockedImage =
          lockedImages."${serviceConfig.locked_image}"
            or (throw "nirion: Image reference ${serviceConfig.locked_image} not found");
      in
      (builtins.removeAttrs serviceConfig [
        "locked_image"
      ])
      // {
        image = lockedImage;
      }
    else
      serviceConfig;

  applyTransforms =
    transforms: projectName: serviceName: serviceConfig:
    lib.lists.foldl (
      svcConfig: transform: transform projectName serviceName svcConfig
    ) serviceConfig transforms;

  transformService = applyTransforms [
    imageTransform
    lockedImageTransform
  ];
in
lib.mapAttrs (
  projectName: projectConfig:
  let
    services = projectConfig.settings.services or { };
    transform = transformService projectName;
    transformedServices = lib.mapAttrs (
      serviceName: serviceConfig:
      if builtins.hasAttr "service" serviceConfig then
        serviceConfig
        // {
          service = transform serviceName serviceConfig.service;
        }
      else
        serviceConfig
    ) services;
  in
  projectConfig
  // {
    settings = (projectConfig.settings or { }) // {
      services = transformedServices;
    };
  }
) nirionConfig.projects
