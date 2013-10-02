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
#import "AFPropertyListRequestOperation.h"

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
        self.putioController.message.stringValue = NSLocalizedString(@"HELPER_AUTH_REQUIRED", nil);
        self.putioController.userInfo.stringValue = self.putioController.message.stringValue;
        self.putioController.transferInfo.stringValue = @"Please log into put.io";
        
        self.putioController.authWindow = [[PutioBrowser alloc] initWithWindowNibName:@"Browser"];
        self.putioController.authWindow.window.level = kCGPopUpMenuWindowLevel;
        [self.putioController.authWindow.window makeMainWindow];
        [self.putioController.toggleShowTransfers setEnabled:NO];
        [NSApp activateIgnoringOtherApps:YES];
    }
    else
    {
        [self.putioAPI setApiToken: token];
        [self updateUserInfo];
        [self.putioController.toggleShowTransfers setEnabled:YES];
        
        [self startUserinfoTimer];
    }
}


- (void)startUserinfoTimer
{
    userInfoTimer = [NSTimer scheduledTimerWithTimeInterval:20.0  target:self selector:@selector(updateUserInfo) userInfo:nil repeats:YES];
}


- (void)updateUserInfo
{
    [self.putioAPI getAccount:^(PKAccount *account)
    {
        putioController.userInfo.stringValue = [NSString stringWithFormat:NSLocalizedString(@"HELPER_USERINFO_ACCOUNT", nil),
            [account username],
            [self transformedValue:[account diskUsed]],
            [self transformedValue:[account diskSize]]
        ];
    }
    failure:^(NSError *error)
    {
        putioController.userInfo.stringValue = NSLocalizedString(@"HELPER_USERINFO_FAILED", nil);
    }];
    
    [self.putioAPI getTransfers:^(NSArray *putioTransfers)
    {
        [self.putioController setTransfers: [putioTransfers mutableCopy]];
        [self.putioController.tableView reloadData];

        int pendingDownloads = 0;
        int completedDownloads = 0;
        int totalTransfers = (int)putioTransfers.count;
        NSString *status;

        for (int i = 0; i < totalTransfers; i++)
        {
            status = [[putioTransfers objectAtIndex:i] valueForKey:@"status"];

            if ([status isEqualToString:@"WAITING"] || [status isEqualToString:@"DOWNLOADING"] || [status isEqualToString:@"IN_QUEUE"])
            {
                pendingDownloads++;
            }
            else if ([status isEqualToString:@"COMPLETED"] || [status isEqualToString:@"SEEDING"])
            {
                completedDownloads++;
            }
        }

        if (pendingDownloads == 0)
        {
            putioController.transferInfo.stringValue = [NSString stringWithFormat:NSLocalizedString(@"HELPER_NO_PENDING_TRANSFERS", nil), completedDownloads];
        }
        else if (pendingDownloads == 1)
        {
            putioController.transferInfo.stringValue = [NSString stringWithFormat:NSLocalizedString(@"HELPER_PENDING_TRANSFERS_SINGULAR", nil), completedDownloads];
        }
        else
        {
            putioController.transferInfo.stringValue = [NSString stringWithFormat:NSLocalizedString(@"HELPER_PENDING_TRANSFERS_PURAL", nil), pendingDownloads, completedDownloads];
        }
    }
    failure:^(NSError *error)
    {
        putioController.userInfo.stringValue = NSLocalizedString(@"HELPER_TRANSFERS_FAILED", nil);
    }];
}


- (void)addMagnet:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent
{
    [closeTimer invalidate];
    
    NSError *error = nil;
    NSString *magnetURL = [[event paramDescriptorForKeyword:keyDirectObject] stringValue];
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
    
    putioController.message.stringValue = [NSString stringWithFormat:NSLocalizedString(@"HELPER_MAGNET_ADDING", nil), displayName];
    [putioController.activityIndicator startAnimation:nil];
    
    [self.putioAPI requestTorrentOrMagnetURLAtPath:magnetURL :^(id userInfoObject)
    {
        if ([[NSUserDefaults standardUserDefaults] boolForKey:@"close.magnet"])
        {
            putioController.message.stringValue = NSLocalizedString(@"HELPER_MAGNET_ADDED_CLOSING", nil);            
            closeTimer = [NSTimer scheduledTimerWithTimeInterval:5.0  target:[NSApplication sharedApplication] selector:@selector(terminate:) userInfo:nil repeats:NO];
        }
        else
        {
            putioController.message.stringValue = NSLocalizedString(@"HELPER_MAGNET_ADDED", nil);
        }
       
        [self updateUserInfo];
        [putioController.activityIndicator stopAnimation:nil];
    }
    addFailure:^
    {
        putioController.message.stringValue = NSLocalizedString(@"HELPER_MAGNET_ERROR", nil);
        [putioController.activityIndicator stopAnimation:nil];
    }
    networkFailure:^(NSError *error)
    {
        // Put.io returns HTTP 400 if you're adding an URL that's already in the queue...
        // ... and thereby causing AFNetworking to throw a network error.
        putioController.message.stringValue = NSLocalizedString(@"HELPER_MAGNET_DUPLICATE", nil);
        [putioController.activityIndicator stopAnimation:nil];
    }];
}


- (void)uploadTorrent:(NSString*)filePath
{
    [closeTimer invalidate];
    //[userInfoTimer invalidate];
    
    NSString *fileName = [filePath lastPathComponent];
    putioController.message.stringValue = [NSString stringWithFormat: NSLocalizedString(@"HELPER_TORRENT_UPLOADING", nil), [fileName displayNameString]];
    [putioController.activityIndicator startAnimation:nil];
    
    [self.putioAPI uploadFile:filePath :^(id userInfoObject)
    {
        if ([[NSUserDefaults standardUserDefaults] boolForKey:@"close.torrent"])
        {
            putioController.message.stringValue = NSLocalizedString(@"HELPER_TORRENT_ADDED_CLOSING", nil);
            closeTimer = [NSTimer scheduledTimerWithTimeInterval:5.0  target:[NSApplication sharedApplication] selector:@selector(terminate:) userInfo:nil repeats:NO];
        }
        else
        {
            putioController.message.stringValue = NSLocalizedString(@"HELPER_TORRENT_ADDED", nil);
        }
        
        [self updateUserInfo];
        [putioController.activityIndicator stopAnimation:nil];
    }
    addFailure:^
    {
        putioController.message.stringValue = NSLocalizedString(@"HELPER_MAGNET_ERROR", nil);
        [putioController.activityIndicator stopAnimation:nil];
    }
    networkFailure:^(NSError *error)
    {
        // Put.io returns HTTP 400 if you're adding an URL that's already in the queue...
        // ... and thereby causing AFNetworking to throw a network error.
        putioController.message.stringValue = NSLocalizedString(@"HELPER_TORRENT_DUPLICATE", nil);
        [putioController.activityIndicator stopAnimation:nil];
    }];
}


- (void)checkForUpdates
{
    NSURL *url = [NSURL URLWithString:@"https://raw.github.com/nicoSWD/put.io-adder/master/put.io%20adder/put.io%20adder-Info.plist"];
    NSURLRequest *request = [NSURLRequest requestWithURL:url];
    
    // GitHub doesn't send the correct x-plist content-type header.
    [AFPropertyListRequestOperation addAcceptableContentTypes:[NSSet setWithObject:@"text/plain"]];
    AFPropertyListRequestOperation *operation = [AFPropertyListRequestOperation propertyListRequestOperationWithRequest:request
    success:^(NSURLRequest *request, NSHTTPURLResponse *response, id propertyList)
    {
        NSString *latestVersion = [propertyList valueForKey:@"CFBundleShortVersionString"];
        NSString *currentVersion = [[[NSBundle mainBundle] infoDictionary] objectForKey:@"CFBundleShortVersionString"];
        
        if (![latestVersion isEqualToString:currentVersion])
        {
            [closeTimer invalidate];
            
            NSAlert *alert = [[NSAlert alloc] init];
            [alert setAlertStyle:NSWarningAlertStyle];
            [alert addButtonWithTitle:NSLocalizedString(@"HELPER_OKAY", nil)];
            [alert addButtonWithTitle:NSLocalizedString(@"HELPER_CANCEL", nil)];
            [alert setMessageText:NSLocalizedString(@"HELPER_NEW_VERSION_TITLE", nil)];
            [alert setInformativeText:[NSString stringWithFormat:NSLocalizedString(@"HELPER_NEW_VERSION_BODY", nil), latestVersion]];
            [alert setShowsSuppressionButton:YES];
            
            if ([alert runModal] == NSAlertFirstButtonReturn)
            {
                [self.putioController openGithub:nil];
            }
            
            if ([[alert suppressionButton] state] == NSOnState)
            {
                NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
                [defaults setBool:NO forKey:@"checkupdate"];
                [defaults synchronize];
            }
        }
    }
    failure:^(NSURLRequest *request, NSHTTPURLResponse *response, NSError *error, id propertyList)
    {
        NSLog(@"Error: %@", error);
    }];

    [operation start];
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