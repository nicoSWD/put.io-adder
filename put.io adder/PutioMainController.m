//
//  PutioMainController.m
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioMainController.h"
#import "AFNetworking.h"
#import "AFJSONRequestOperation.h"
#import "PutioBrowser.h"
#import "SSKeychain.h"
#import "V2PutIOAPIClient.h"
#import "NSString+DisplayName.h"

@implementation PutioMainController

@synthesize message, activityIndicator, authWindow, oauthToken, userInfo, versionInfo, transferInfo, putioAPI;


-(void)awakeFromNib
{
    [[NSAppleEventManager sharedAppleEventManager] setEventHandler:self andSelector:@selector(addMagnet:withReplyEvent:) forEventClass:kInternetEventClass andEventID:kAEGetURL];
    
    self.versionInfo.stringValue = [NSString stringWithFormat:@"v%@", [[[NSBundle mainBundle] infoDictionary] objectForKey:@"CFBundleShortVersionString"]];
    self.putioAPI = [V2PutIOAPIClient setup];
    [self authenticateUser];
}


- (void)authenticateUser
{
    NSError *error;
    NSString *token = [SSKeychain passwordForService:@"put.io adder" account:@"711" error:&error];
    
    if ([error code] == SSKeychainErrorNotFound)
    {
        self.message.stringValue = @"Authentication required!";
        
        self.authWindow = [[PutioBrowser alloc] initWithWindowNibName:@"Browser"];
        [self.authWindow.window makeKeyWindow];
        [self.authWindow.window makeMainWindow];
        
        /////////////////////
        // TODO: move auth window in front of *ALL* other windows.
        // Seems simple enough, but I can't get it to work.
        //
        // Pull request, anyone?
        /////////////////////
    }
    else
    {
        
        self.putioAPI.apiToken = token;
        self.oauthToken = token;
        [self updateUserInfo];
        
        userInfoTimer = [NSTimer scheduledTimerWithTimeInterval:30.0  target:self selector:@selector(updateUserInfo) userInfo:nil repeats:YES];
    }
}


- (void)updateUserInfo
{  
    [self.putioAPI getAccount:^(PKAccount *account)
    {       
        self.userInfo.stringValue = [NSString stringWithFormat:@"%@, %@ used of %@",
             [account username],
             [self transformedValue:[account diskUsed]],
             [self transformedValue:[account diskSize]]
        ];
    }
    failure:^(NSError *error)
    {
        self.userInfo.stringValue = @"Failed to fetch account info!";
    }];
    
    [self.putioAPI getTransfers:^(NSArray *transfers)
    {
        int pendingDownloads = 0;
        NSString *status;
        
        for (int i = 0; i < transfers.count; i++)
        {
            status = [[transfers objectAtIndex:i] valueForKey:@"status"];

            if ([status isEqualToString:@"WAITING"] || [status isEqualToString:@"DOWNLOADING"])
            {
                pendingDownloads++;
            }
        }
        
        if (pendingDownloads > 0)
        {
            self.transferInfo.stringValue = [NSString stringWithFormat:@"%i pending transfer(s)", pendingDownloads];
        }
        else
        {
            self.transferInfo.stringValue = @"No pending transfers!";
        }
    }
    failure:^(NSError *error)
    {
        self.userInfo.stringValue = @"Failed to fetch transfers!";
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
        displayName = [magnetURL substringWithRange:[[matches objectAtIndex:0] rangeAtIndex:1]];
        displayName = [displayName displayNameString];
    }
    else
    {
        displayName = magnetURL;
    }
    
    self.message.stringValue = [NSString stringWithFormat:@"Adding magnet: %@", displayName];
    [self.activityIndicator startAnimation:nil];

    [self.putioAPI requestTorrentOrMagnetURLAtPath:magnetURL :^(id userInfoObject)
    {
        self.message.stringValue = @"URL successfully added!";
        [self.activityIndicator stopAnimation:nil];
    }
    addFailure:^
    {
        self.message.stringValue = @"Something went wrong!?";
        [self.activityIndicator stopAnimation:nil];
    }
    networkFailure:^(NSError *error)
    {
        // Put.io returns HTTP 400 if you're adding an URL that's already in the queue...
        // ... and thereby causing AFNetworking to throw a network error.
        self.message.stringValue = @"Failed! URL already in queue?";
        [self.activityIndicator stopAnimation:nil];
    }];
}


- (void)uploadTorrent:(NSString*)filePath
{
    NSURL *baseURL = [NSURL URLWithString:@"https://api.put.io/"];
    NSData *imageData = [NSData dataWithContentsOfFile:filePath];
    NSString *fileName = [filePath lastPathComponent];
    NSDictionary *params = [NSDictionary dictionaryWithObjectsAndKeys: self.oauthToken, @"oauth_token", nil];
    
    self.message.stringValue = [NSString stringWithFormat:@"Uploading .torrent: %@", fileName];
    [self.activityIndicator startAnimation:nil];

    AFHTTPClient *httpClient = [[AFHTTPClient alloc] initWithBaseURL:baseURL];
    NSURLRequest *request = [httpClient multipartFormRequestWithMethod:@"POST" path:@"/v2/files/upload" parameters:params constructingBodyWithBlock: ^(id <AFMultipartFormData> formData)
    {
        [formData appendPartWithFileData:imageData name:@"file" fileName:fileName mimeType:@"application/x-bittorrent"];
    }];

    AFJSONRequestOperation *operation = [AFJSONRequestOperation JSONRequestOperationWithRequest:request
    success:^(NSURLRequest *request, NSHTTPURLResponse *response, id JSON)
    {
        if ([[JSON valueForKeyPath:@"status"] isEqualToString:@"OK"])
        {
            self.message.stringValue = @"Torrent successfully added!";
        }
        else
        {
            self.message.stringValue = @"Something went wrong!";
        }

        [self.activityIndicator stopAnimation:nil];
    }
    failure:^(NSURLRequest *request, NSHTTPURLResponse *response, NSError *error, id JSON)
    {
        long statusCode = (long)response.statusCode;

        if (statusCode == 400)
        {
            self.message.stringValue = @"Failed! Torrent already in queue?";
        }
        else
        {
            self.message.stringValue = [NSString stringWithFormat:@"Something went wrong! HTTP error %li!", statusCode];
        }

        [self.activityIndicator stopAnimation:nil];
    }];
    
    [operation start];
}


/**
 * transformedValue method by "Parag Bafna", from: http://stackoverflow.com/questions/7846495/
 *
 **/
- (id)transformedValue:(id)value
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


- (IBAction)loadWebsite:(id)sender
{
	[[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:@"https://github.com/nicoSWD/put.io-adder"]];
}

@end