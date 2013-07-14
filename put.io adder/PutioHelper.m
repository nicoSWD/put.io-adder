//
//  PutioHelper.m
//  put.io adder
//
//  Created by Nico on 7/13/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioHelper.h"
#import "PutioMainController.h"
#import "SSKeychain.h"
#import "V2PutIOAPIClient.h"
#import "NSString+DisplayName.h"

@implementation PutioHelper

@synthesize putioController, putioAPI;

static PutioHelper *sharedHelper = nil;


+ (PutioHelper*)sharedHelper
{
    @synchronized(self)
    {
        static dispatch_once_t onceToken;
        dispatch_once(&onceToken, ^{
            sharedHelper = [[self alloc] init];
            sharedHelper.putioAPI = [V2PutIOAPIClient setup];
            sharedHelper.putioController = [[[[NSApplication sharedApplication] windows] objectAtIndex:0] windowController];
        });
    }
    
    return sharedHelper;
}


- (id)init
{
    return self;
}


- (void)authenticateUser
{
    NSError *error;
    NSString *token = [SSKeychain passwordForService:@"put.io adder" account:@"711" error:&error];
    if ([error code] == SSKeychainErrorNotFound)
    {
        putioController.message.stringValue = @"Authentication required!";
        
        putioController.authWindow = [[PutioBrowser alloc] initWithWindowNibName:@"Browser"];
        putioController.authWindow.window.level = kCGPopUpMenuWindowLevel;
        [putioController.authWindow showWindow:nil];
        [putioController.authWindow.window makeKeyWindow];
        [putioController.authWindow.window makeMainWindow];
    }
    else
    {
        self.putioAPI.apiToken = token;
        [self updateUserInfo];
        
        userInfoTimer = [NSTimer scheduledTimerWithTimeInterval:20.0  target:self selector:@selector(updateUserInfo) userInfo:nil repeats:YES];
    }
}


- (void)updateUserInfo
{
    [self.putioAPI getAccount:^(PKAccount *account)
    {
        putioController.userInfo.stringValue = [NSString stringWithFormat:@"%@, %@ used of %@",
            [account username],
            [self transformedValue:[account diskUsed]],
            [self transformedValue:[account diskSize]]
        ];
    }
    failure:^(NSError *error)
    {
        putioController.userInfo.stringValue = @"Failed to fetch account info!";
    }];
    
    [self.putioAPI getTransfers:^(NSArray *transfers)
    {
        int pendingDownloads = 0;
        int totalTransfers = (int)transfers.count;
        NSString *status;

        for (int i = 0; i < totalTransfers; i++)
        {
            status = [[transfers objectAtIndex:i] valueForKey:@"status"];

            if ([status isEqualToString:@"WAITING"] || [status isEqualToString:@"DOWNLOADING"])
            {
                pendingDownloads++;
            }
        }

        if (pendingDownloads > 0)
        {
            putioController.transferInfo.stringValue = [NSString stringWithFormat:@"%i pending transfer(s)", pendingDownloads];
        }
        else
        {
            putioController.transferInfo.stringValue = @"No pending transfers!";
        }
    }
    failure:^(NSError *error)
    {
        putioController.userInfo.stringValue = @"Failed to fetch transfers!";
    }];
}


- (void)addMagnet:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent
{
    NSString *magnetURL = [[event paramDescriptorForKeyword:keyDirectObject] stringValue];
    NSError *error = nil;
    NSRegularExpression* regex = [NSRegularExpression regularExpressionWithPattern:@"dn=([^&]{5,})" options:0 error:&error];
    NSArray* matches = [regex matchesInString:magnetURL options:0 range:NSMakeRange(0, [magnetURL length])];
    NSString *displayName;
    
    if ([matches count] > 0)
    {
        displayName = [[magnetURL substringWithRange:[[matches objectAtIndex:0] rangeAtIndex:1]] displayNameString];
    }
    else
    {
        displayName = magnetURL;
    }
    
    putioController.message.stringValue = [NSString stringWithFormat:@"Adding magnet: %@", displayName];
    [putioController.activityIndicator startAnimation:nil];
    
    [self.putioAPI requestTorrentOrMagnetURLAtPath:magnetURL :^(id userInfoObject)
    {
        putioController.message.stringValue = @"URL successfully added!";
        [putioController.activityIndicator stopAnimation:nil];
    }
    addFailure:^
    {
        putioController.message.stringValue = @"Something went wrong!?";
        [putioController.activityIndicator stopAnimation:nil];
    }
    networkFailure:^(NSError *error)
    {
        // Put.io returns HTTP 400 if you're adding an URL that's already in the queue...
        // ... and thereby causing AFNetworking to throw a network error.
        putioController.message.stringValue = @"Failed! URL already in queue?";
        [putioController.activityIndicator stopAnimation:nil];
    }];
}


- (void)uploadTorrent:(NSString*)filePath
{
    NSString *fileName = [filePath lastPathComponent];
    putioController.message.stringValue = [NSString stringWithFormat:@"Uploading torrent: %@", [fileName displayNameString]];
    [putioController.activityIndicator startAnimation:nil];
    
    [self.putioAPI uploadFile:filePath :^(id userInfoObject)
    {
        if ([[NSUserDefaults standardUserDefaults] boolForKey:@"CloseAfterSaving"])
        {
            putioController.message.stringValue = @"Torrent successfully added, closing in 5 seconds!";
            [putioController.activityIndicator stopAnimation:nil];
            [[NSApplication sharedApplication] performSelector:@selector(terminate:) withObject:self afterDelay:5];
        }
        else
        {
            putioController.message.stringValue = @"Torrent successfully added!";
            [putioController.activityIndicator stopAnimation:nil];
        }
    }
    addFailure:^
    {
        putioController.message.stringValue = @"Something went wrong!?";
        [putioController.activityIndicator stopAnimation:nil];
    }
    networkFailure:^(NSError *error)
    {
        // Put.io returns HTTP 400 if you're adding an URL that's already in the queue...
        // ... and thereby causing AFNetworking to throw a network error.
        putioController.message.stringValue = @"Failed! Torrent already in queue?";
        [putioController.activityIndicator stopAnimation:nil];
    }];
}


/**
 * transformedValue method by "Parag Bafna", from: http://stackoverflow.com/questions/7846495/
 *
 **/
- (NSString*)transformedValue:(id)value
{
    double convertedValue = [value doubleValue];
    int multiplyFactor = 0;
    
    NSArray *tokens = [NSArray arrayWithObjects:@"bytes",@"KiB",@"MiB",@"GiB",@"TiB",nil];
    
    while (convertedValue > 1024)
    {
        convertedValue /= 1024;
        multiplyFactor++;
    }
    
    return [NSString stringWithFormat:@"%4.2f %@", convertedValue, [tokens objectAtIndex:multiplyFactor]];
}


@end
